//! Authoritative game server. Owns the world, ticks at a fixed rate, and
//! streams binary snapshots to any connected client over WebSocket.

mod world;

use std::{net::SocketAddr, time::Duration};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMsg, EntityId, ServerMsg, PROTOCOL_VERSION};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::MissedTickBehavior,
};
use world::{World, TICK_RATE_HZ};

/// Things connection tasks tell the game loop. The loop is the only thing
/// that ever touches `World`, so there are no locks.
enum Event {
    Join(oneshot::Sender<EntityId>),
    Input { id: EntityId, buttons: u8 },
    Leave(EntityId),
}

#[derive(Clone)]
struct AppState {
    events: mpsc::Sender<Event>,
    snapshots: broadcast::Sender<Vec<u8>>,
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let (events_tx, events_rx) = mpsc::channel(1024);
    let (snap_tx, _) = broadcast::channel(16);

    tokio::spawn(game_loop(events_rx, snap_tx.clone()));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(AppState { events: events_tx, snapshots: snap_tx });

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("failed to bind");
    println!("engine listening on ws://{addr}/ws  ({TICK_RATE_HZ} Hz)");
    axum::serve(listener, app).await.expect("server error");
}

async fn game_loop(mut events: mpsc::Receiver<Event>, snapshots: broadcast::Sender<Vec<u8>>) {
    let mut world = World::new();
    let mut ticker = tokio::time::interval(Duration::from_secs_f64(1.0 / TICK_RATE_HZ as f64));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        while let Ok(ev) = events.try_recv() {
            match ev {
                Event::Join(reply) => {
                    let _ = reply.send(world.add_player());
                }
                Event::Input { id, buttons } => world.set_buttons(id, buttons),
                Event::Leave(id) => world.remove_player(id),
            }
        }

        world.step();

        if snapshots.receiver_count() > 0 {
            let _ = snapshots.send(world.snapshot().encode());
        }
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (reply_tx, reply_rx) = oneshot::channel();
    if state.events.send(Event::Join(reply_tx)).await.is_err() {
        return;
    }
    let Ok(id) = reply_rx.await else { return };
    println!("player {id} joined");

    let (mut sink, mut stream) = socket.split();
    let hello = ServerMsg::Hello { version: PROTOCOL_VERSION, player_id: id }.encode();
    if sink.send(Message::Binary(hello)).await.is_ok() {
        let mut snaps = state.snapshots.subscribe();
        loop {
            tokio::select! {
                snap = snaps.recv() => match snap {
                    Ok(bytes) => {
                        if sink.send(Message::Binary(bytes)).await.is_err() { break; }
                    }
                    // Client is too slow: skip ahead, only the latest state matters.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                msg = stream.next() => match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Ok(ClientMsg::Input { buttons }) = ClientMsg::decode(&data) {
                            let _ = state.events.send(Event::Input { id, buttons }).await;
                        }
                        // Malformed messages are ignored rather than trusted.
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => {}
                },
            }
        }
    }

    let _ = state.events.send(Event::Leave(id)).await;
    println!("player {id} left");
}
