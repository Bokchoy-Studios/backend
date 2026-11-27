//! Pure game state and rules. No networking, no rendering: easy to test.

use protocol::{button, kind, EntityId, EntityState, ServerMsg};
use std::collections::BTreeMap;

pub const TICK_RATE_HZ: u32 = 60;
pub const DT: f32 = 1.0 / TICK_RATE_HZ as f32;
/// Tiles per second.
pub const MOVE_SPEED: f32 = 4.0;
/// The arena spans -ARENA_HALF_SIZE..ARENA_HALF_SIZE on both axes.
pub const ARENA_HALF_SIZE: f32 = 16.0;

struct Cube {
    x: f32,
    y: f32,
    buttons: u8,
}

pub struct World {
    tick: u32,
    next_id: EntityId,
    cubes: BTreeMap<EntityId, Cube>, // BTreeMap: stable ordering in snapshots
}

impl World {
    pub fn new() -> Self {
        Self { tick: 0, next_id: 1, cubes: BTreeMap::new() }
    }

    pub fn add_player(&mut self) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        self.cubes.insert(id, Cube { x: 0.0, y: 0.0, buttons: 0 });
        id
    }

    pub fn remove_player(&mut self, id: EntityId) {
        self.cubes.remove(&id);
    }

    pub fn set_buttons(&mut self, id: EntityId, buttons: u8) {
        if let Some(c) = self.cubes.get_mut(&id) {
            c.buttons = buttons;
        }
    }

    /// Advance the simulation by one fixed tick.
    pub fn step(&mut self) {
        for c in self.cubes.values_mut() {
            let dx = axis(c.buttons, button::RIGHT, button::LEFT);
            let dy = axis(c.buttons, button::UP, button::DOWN);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.0 {
                // Normalise so diagonals aren't faster.
                c.x += dx / len * MOVE_SPEED * DT;
                c.y += dy / len * MOVE_SPEED * DT;
            }
            c.x = c.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
            c.y = c.y.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
        }
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn snapshot(&self) -> ServerMsg {
        ServerMsg::Snapshot {
            tick: self.tick,
            entities: self
                .cubes
                .iter()
                .map(|(&id, c)| EntityState { id, kind: kind::CUBE, x: c.x, y: c.y })
                .collect(),
        }
    }
}

fn axis(buttons: u8, pos: u8, neg: u8) -> f32 {
    (buttons & pos != 0) as i8 as f32 - (buttons & neg != 0) as i8 as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(buttons: u8, ticks: u32) -> (f32, f32) {
        let mut w = World::new();
        let id = w.add_player();
        w.set_buttons(id, buttons);
        for _ in 0..ticks {
            w.step();
        }
        let c = &w.cubes[&id];
        (c.x, c.y)
    }

    #[test]
    fn walks_right_one_second() {
        let (x, y) = run(button::RIGHT, TICK_RATE_HZ);
        assert!((x - MOVE_SPEED).abs() < 1e-3);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn up_is_positive_y() {
        let (_, y) = run(button::UP, 10);
        assert!(y > 0.0);
    }

    #[test]
    fn diagonal_is_not_faster() {
        let (x, y) = run(button::UP | button::RIGHT, TICK_RATE_HZ);
        let dist = (x * x + y * y).sqrt();
        assert!((dist - MOVE_SPEED).abs() < 1e-3);
    }

    #[test]
    fn opposite_keys_cancel() {
        assert_eq!(run(button::LEFT | button::RIGHT, 30), (0.0, 0.0));
    }

    #[test]
    fn stays_inside_arena() {
        let (x, _) = run(button::RIGHT, TICK_RATE_HZ * 60);
        assert_eq!(x, ARENA_HALF_SIZE);
    }

    #[test]
    fn snapshot_lists_players_and_drops_leavers() {
        let mut w = World::new();
        let a = w.add_player();
        let b = w.add_player();
        w.remove_player(a);
        match w.snapshot() {
            ServerMsg::Snapshot { entities, .. } => {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].id, b);
            }
            _ => unreachable!(),
        }
    }
}
