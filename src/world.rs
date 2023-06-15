use std::{time::{Instant, Duration}, sync::{mpsc::Sender, Arc}, f32::consts::PI};

use cgmath::{Matrix4, Rad, Vector3, Deg, Matrix3, Bounded, InnerSpace, num_traits::clamp};
use winit::event::VirtualKeyCode;

use self::{player::{Player, PlayerPosition}, controls::{Controller, Control}, chunkedterrain::ChunkedTerrain, chunk_worker_pool::ChunkTask, chunk::Chunk};

mod block;
mod player;
mod controls;
pub mod chunkedterrain;
pub mod chunk;
pub mod chunk_worker_pool;


const MOUSE_SENS: Rad<f32> = Rad(0.002); //Rads per dot.
const NOCLIP_SPEED: f32 = 40.0; //blocks/sec

pub struct World {
  terrain: ChunkedTerrain,
  player: Player,
  last_tick: Instant,
  controller: Controller,
  uptime: Duration
}


impl World {
  pub fn new(worker_pool_sender: Sender<ChunkTask>, chunk_gc: Sender<Arc<Chunk>>) -> Self {
    let player_pos = PlayerPosition {
        block_int: [1, 40, 1].into(),
        block_dec: [0.0; 3].into(),
    };
    
    let player = Player::new(player_pos.into());
    
    let terrain = ChunkedTerrain::new(player_pos, 8, worker_pool_sender, chunk_gc);
    let last_tick = Instant::now();
    let mut controller = Controller::new();

    controller.set_bindings(&[
      (VirtualKeyCode::W, Control::Forward),
      (VirtualKeyCode::A, Control::Left),
      (VirtualKeyCode::S, Control::Backward),
      (VirtualKeyCode::D, Control::Right),
      (VirtualKeyCode::Space, Control::Up),
      (VirtualKeyCode::LControl, Control::Down),
      (VirtualKeyCode::Up, Control::Forward),
      (VirtualKeyCode::Left, Control::Left),
      (VirtualKeyCode::Down, Control::Backward),
      (VirtualKeyCode::Right, Control::Right),
      (VirtualKeyCode::RShift, Control::Up),
      (VirtualKeyCode::RControl, Control::Down)
    ]);

    let uptime = Duration::new(0, 0);
    
    Self {
      terrain,
      player,
      last_tick,
      controller,
      uptime
    }
  }

  pub fn get_player_view(&self, aspect_ratio: f32) -> Matrix4<f32> {
    let delta_t = self.since_last_tick();
    self.player.get_view_matrix(aspect_ratio, delta_t)
  }

  pub fn get_player_pos(&self) -> PlayerPosition {
    self.player.get_position()
  }

  pub fn mouse_move(&mut self, delta: (f64, f64)) {
    self.player.rotate_camera(MOUSE_SENS * delta.0 as f32, MOUSE_SENS * delta.1 as f32);
  }

  pub fn tick(&mut self) {
    let delta_secs = self.since_last_tick();
    self.uptime += delta_secs;

    self.last_tick = Instant::now();

    let x_speed = self.controller.get_action_value((Control::Left, -1.0), (Control::Right, 1.0), 0.0);
    let y_speed = self.controller.get_action_value((Control::Down, -1.0), (Control::Up, 1.0), 0.0);
    let z_speed = self.controller.get_action_value((Control::Backward, -1.0), (Control::Forward, 1.0), 0.0);

    let direction_vector = Vector3 {
      x: x_speed,
      y: y_speed,
      z: z_speed
    };

    let accel = self.player.get_rotation_matrix() * direction_vector * NOCLIP_SPEED ;
    self.player.tick_position(&accel, &delta_secs);

    
    self.terrain.update_player_position(&self.player.get_position());
    self.terrain.tick_progress();
  }

  pub fn key_update(&mut self, key: VirtualKeyCode, state: bool) {
    self.controller.set_key(key, state);
  }

  pub fn get_terrain(&self) -> &ChunkedTerrain {
    &self.terrain
  }

  pub fn get_daylight_data(&self) -> WorldLightData {
    const DAY_CYCLE_TIME: f32 = 300.0; //300 seconds
    const TILT: Deg<f32> = Deg(40.0); //20 degree tilt from horizon

    let cycle = ((self.uptime + self.since_last_tick()).as_secs_f32() % DAY_CYCLE_TIME) / DAY_CYCLE_TIME;
    
    let rotation =  Matrix3::from_angle_z(TILT) * Matrix3::from_angle_y(Rad(2.0 * PI) * cycle);

    let sun_direction = rotation * Vector3 {x: 1.0, y: 0.0, z: 0.0};

    let sun_angle = Rad(sun_direction.y.asin());

    let light_level = clamp((sun_angle / Rad::<f32>::from(TILT)) + 1.0, 0.1, 1.0);

    WorldLightData { sun_direction, light_level }
  }

  fn since_last_tick(&self) -> Duration {
    let now = Instant::now();
    now - self.last_tick
  }
}

pub struct WorldLightData {
  pub sun_direction: Vector3<f32>,
  pub light_level: f32
}