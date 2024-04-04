use std::{f32::consts::PI, ops::AddAssign, time::Duration};

use cgmath::{Matrix4, Rad, Deg, Matrix3, Point3, num_traits::clamp, Vector3, EuclideanSpace};

use crate::util::{projection, FPVector, Fixed64};

use super::chunkedterrain::ChunkedTerrain;

const SPEED_FACTOR: f32 = 0.5;
const DEFAULT_FOV: f32 = 75.0;

const DEFAULT_HITBOX: HitBox = HitBox {
  // lo: [-0.5, -1.5, -0.5].into(),
  // hi: [0.5, 0.5, 0.5].into()

  //WARNING: DO NOT USE RECURRING DECIMALS.
  lo: FPVector { inner: Vector3 { x: Fixed64::lit("-0.5"), y: Fixed64::lit("-1.5"), z: Fixed64::lit("-0.5") } },
  hi: FPVector { inner: Vector3 { x: Fixed64::lit("0.5"), y: Fixed64::lit("0.5"), z: Fixed64::lit("0.5") } },
};
//TODO possibly split hitbox and position data into a separate entity data structure.

pub struct Player {
  position: FPVector,
  velocity: Vector3<f32>,
  yaw: Rad<f32>,
  pitch: Rad<f32>,
  pub fov: f32,
  hitbox: HitBox
}

/**
 The hitbox is a 3-dimensional cuboid aligned with the world that cannot rotate. It is relevant for player physics.
 */
pub struct HitBox {
  // pub lo: Vector3<f32>,
  // pub hi: Vector3<f32>
  pub lo: FPVector,
  pub hi: FPVector
}

/// The PlayerPosC struct is now only used for passing positional data to the GPU.
#[repr(C)] //Repr(c) because this is being sent to GPU.
#[derive(Debug, Clone, Copy)]
pub struct PlayerPosC {
  pub block_int: Point3<i32>, //The block integer.
  pub block_dec: Point3<f32> //The decimal part.
}

impl Player {
  pub fn new(position: FPVector) -> Self {
    Self {
      position,
      velocity: Vector3 { x: 0.0, y: 0.0, z: 0.0 },
      yaw: Rad(0.0),
      pitch: Rad(0.0),
      fov: DEFAULT_FOV,
      hitbox: DEFAULT_HITBOX
    }
  }

  ///Gets the player view matrix relative to the nearest block. Conversions on integers still need to be done on the GPU.
  pub fn get_view_matrix(&self, aspect_ratio: f32, dt: Duration) -> Matrix4<f32> {
    let rotation = self.get_rotation_matrix();
    // let pos_offset = self.velocity * dt.as_secs_f32(); //To prevent stuttering and lagging on high Hz monitors.
    let pos_offset = Vector3::from([0.0; 3]); //TODO temp.
    let view = Matrix4::look_to_lh(Point3::from_vec(/*self.position.get_dec() + */ pos_offset), rotation.z, rotation.y);
    let projection = projection(Deg(self.fov), aspect_ratio, 0.1, 400.0); //Very very far far plane.

    projection * view
  }

  pub fn rotate_camera(&mut self, dx: impl Into<Rad<f32>>, dy: impl Into<Rad<f32>>) {
    let dxr = dx.into();
    let dyr = dy.into();

    self.yaw = (self.yaw + dxr) % Rad(2.0 * PI);
    self.pitch = clamp(self.pitch + dyr, Rad(-PI/2.0), Rad(PI/2.0));
  }

  pub fn get_rotation_matrix(&self) -> Matrix3<f32> {
    Matrix3::from_angle_y(self.yaw) * Matrix3::from_angle_x(self.pitch)
  }

  pub fn get_position(&self) -> FPVector{
    self.position
  }

  pub fn get_pos_c(&self) -> PlayerPosC {
    PlayerPosC {
        block_int: Point3::from_vec(self.position.get_int()),
        block_dec: Point3::from_vec(self.position.get_dec()),
    }
  }

  /**
  Update player position in world.
   - `target_vel` - Target velocity.
   - `dt` - Duration since last tick.
   - `terrain` - World terrain data. 
   */
  pub fn tick_position(&mut self, target_vel: &Vector3<f32>, dt: &Duration, terrain: &ChunkedTerrain) {
    let diff = target_vel - self.velocity;
    let secs = dt.as_secs_f32();
    let factor = secs/(secs + SPEED_FACTOR);
    self.velocity += diff * factor;
    
    terrain.update_collision_info(&mut self.position, &mut self.velocity, secs, &self.hitbox);
  }


}

impl AddAssign<Vector3<f32>> for PlayerPosC {
  fn add_assign(&mut self, rhs: Vector3<f32>) {
      let added_float = self.block_dec + rhs;
      self.block_int = self.block_int.zip(added_float, |s, t| s + t.trunc() as i32 + (if t<0.0 {-1} else {0})); //Add integer components.
      self.block_dec = added_float.map(|v| if v<0.0 {v.fract()+1.0} else {v.fract()}); //Add decimal components.
  }
}

// //TODO cleanup messy boilerplate code.
// impl PlayerPosC {
//   //Not through an Into trait as I want to avoid casting to float as much as possible.
//   pub fn as_vec_f32(self) -> Vector3<f32> {
//     self.block_int.map(|v| v as f32).to_vec() + self.block_dec.to_vec()
//   }
// }

// impl Add<Vector3<f32>> for PlayerPosC {
//     type Output = PlayerPosC;

//     fn add(mut self, rhs: Vector3<f32>) -> Self::Output {
//       self += rhs; //Use the AddAssign trait implemented below.
//       self
//     }
// }

// impl SubAssign<Vector3<f32>> for PlayerPosC {
//   fn sub_assign(&mut self, rhs: Vector3<f32>) {
//       *self += -rhs;
//   }
// }

// impl Sub<Vector3<f32>> for PlayerPosC {
//   type Output = PlayerPosC;

//   fn sub(self, rhs: Vector3<f32>) -> Self::Output {
//       self + -rhs
//   }
// }

// impl AddAssign<Vector3<i32>> for PlayerPosC {
//   fn add_assign(&mut self, rhs: Vector3<i32>) {
//     self.block_int += rhs;
//   }
// }

// impl Add<Vector3<i32>> for PlayerPosC {
//   type Output = PlayerPosC;

//   fn add(mut self, rhs: Vector3<i32>) -> Self::Output {
//     self += rhs; //Use AddAssign trait again.
//     self
//   }
// }

// impl SubAssign<Vector3<i32>> for PlayerPosC {
//   fn sub_assign(&mut self, rhs: Vector3<i32>) {
//       *self += -rhs;
//   }
// }

// impl Sub<Vector3<i32>> for PlayerPosC {
//   type Output = PlayerPosC;

//   fn sub(self, rhs: Vector3<i32>) -> Self::Output {
//       self + -rhs
//   }
// }


// #[cfg(test)]
// mod test {
//     use cgmath::{Vector3, Point3, EuclideanSpace, InnerSpace};

//     use super::PlayerPosC;

//   #[test]
//   fn test_pos_add() {
//     let a = PlayerPosC {
//       block_dec: [0.0f32, 0.0, 0.0].into(),
//       block_int: [1i32, 1, 1].into()
//     };

//     let added = a + Vector3::from([1.5f32, 1.5, 1.5]);
    
//     assert!((added.block_dec.to_vec() - Vector3{x:0.5f32,y:0.5,z:0.5}).magnitude() < 0.01);
//     assert_eq!(added.block_int, Point3{x:2i32,y:2,z:2});


//     let neg_add = a + Vector3::from([-0.5f32, -0.5, -0.5]);
//     assert!((neg_add.block_dec.to_vec() - Vector3{x:0.5,y:0.5,z:0.5}).magnitude() < 0.01);
//     assert_eq!(neg_add.block_int, Point3{x:0i32,y:0,z:0});
//   }
// }