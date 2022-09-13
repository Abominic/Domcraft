use std::{sync::{Mutex, Arc, RwLock}, ops::Range};

use bytemuck_derive::{Zeroable, Pod};
use itertools::iproduct;
use noise::{Perlin, NoiseFn};
use wgpu::{Device, Queue};

use crate::{renderer::buffer::{GenericBuffer, GenericBufferType}};

use super::{block::{Block, BlockSideVisibility, BlockSide}, chunkedterrain::{SurfaceHeightmap, CHUNK_LENGTH, CHUNK_SIZE, CHUNK_RANGE}};

pub const ADJACENT_OFFSETS: [[i32; 3]; 6] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1]
];

const CHUNK_RANGE_I32: Range<i32> = 0..CHUNK_SIZE as i32;

pub struct Chunk {
  chunk_id: [i32; 3],
  blocks: RwLock<Option<Vec<Block>>>,
  block_vis: Mutex<Option<Vec<BlockSideVisibility>>>,
  mesh: Mutex<Option<ChunkMesh>>,
  state: Mutex<ChunkState>
}


struct ChunkState {
  stage: ChunkStateStage,
  progress: ChunkStateProgress
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkStateStage {
  ChunkGen,
  ChunkVisGen,
  MeshGen,
  Ready
}


enum ChunkStateProgress {
  Waiting,
  TaskAssigned, //Activated before it is sent to another thread.
  Processing, //When the processing is actually happening.
  SwitchingTo(ChunkStateStage), //Can be used to interrupt the state.
}


struct ChunkMesh {
  vertex_buffer: GenericBuffer<ChunkVertex>,
  index_buffer: GenericBuffer<u32>,
}

pub struct ChunkMeshData {
  pub vertex_buffer: (Arc<wgpu::Buffer>, u64),
  pub index_buffer: (Arc<wgpu::Buffer>, u64),
}

impl Chunk {
  /// Generate a new chunk. 
  pub fn new(chunk_id: [i32; 3]) -> Self {

    Self {
      chunk_id,
      blocks: RwLock::new(None),
      block_vis: Mutex::new(None),
      mesh: Mutex::new(None),
      state: Mutex::new(ChunkState {
        stage: ChunkStateStage::ChunkGen,
        progress: ChunkStateProgress::Waiting,
      })
    }
  }

  ///Check if processing can start. Panics if something bad happens.
  fn start_process_check(&self, expected_stage: ChunkStateStage) -> bool {
    let mut state = self.state.lock().unwrap();
    if state.stage != expected_stage {
      panic!("Function called at wrong stage!!!");
    }
    match state.progress {
      ChunkStateProgress::Waiting => panic!("Chunk was not task assigned!"),
      ChunkStateProgress::TaskAssigned => {
        state.progress = ChunkStateProgress::Processing; 
        true
      },
      ChunkStateProgress::Processing => { //Skip because the chunk is already processing.
        false
      },
      ChunkStateProgress::SwitchingTo(t) => {
        state.stage = t;
        state.progress = ChunkStateProgress::Waiting;
        false
      },
    }
  }

  fn end_process_check<T>(&self, current_stage: ChunkStateStage, next_stage: ChunkStateStage, success: T)
    where T: FnOnce() 
  { //Cursed brackets
    let mut state = self.state.lock().unwrap();
    if state.stage != current_stage {
      panic!("Chunk stage changed mid processing: Expected: {:?}. Got: {:?}", current_stage, state.stage);
    }
    match state.progress {
      ChunkStateProgress::Processing => {
        success(); //Call success function.
        state.stage = next_stage; //Go to next stage;
        state.progress = ChunkStateProgress::Waiting;
      },
      ChunkStateProgress::SwitchingTo(new_state) => {
        state.stage = new_state;
        state.progress = ChunkStateProgress::Waiting;
      },
      _ => panic!("Chunk progress changed to invalid while processing.")
    }
  }

  pub fn gen(&self, gen: &Perlin, surface_heightmap: &SurfaceHeightmap) {
    if !self.start_process_check(ChunkStateStage::ChunkGen) { //Skip if the chunk is not ready to generate.
      return;
    }

    let chunk_pos = self.chunk_id.map(|chk| {
      chk*CHUNK_SIZE as i32
    });


    let mut blocks = Vec::<Block>::with_capacity(CHUNK_LENGTH);
    for (x, y, z) in block_iterator() {
      let surface_level = surface_heightmap[x*CHUNK_SIZE + z];
      let actual_pos = [
        chunk_pos[0] + x as i32,
        chunk_pos[1] + y as i32,
        chunk_pos[2] + z as i32
      ];

      

      let block = if actual_pos[1] > surface_level {
        Block::Air
      } else {
        let noise_value = NoiseFn::<[f64; 3]>::get(gen, actual_pos.map(|val| val as f64 / 60.0));
        let is_cave = noise_value > 0.5;
        if is_cave {
          Block::Air
        } else if actual_pos[1] == surface_level {
          Block::Grass
        } else {
          Block::Stone
        }
      };

      blocks.push(block);
    }

    *self.blocks.write().unwrap() = Some(blocks); //Should move inside success function but oh well.
    self.end_process_check(ChunkStateStage::ChunkGen, ChunkStateStage::ChunkVisGen, || {
      
    });
  }

  /// Gets the block at the chunk-relative location. 
  pub fn get_block_at(&self, x: i32, y: i32, z: i32) -> Option<Block> {
    self.blocks.read().unwrap().as_ref().and_then(|blocks| {
      if CHUNK_RANGE_I32.contains(&x) && CHUNK_RANGE_I32.contains(&y) && CHUNK_RANGE_I32.contains(&z) {
        Some(*blocks.get(x as usize * CHUNK_SIZE * CHUNK_SIZE + y as usize * CHUNK_SIZE + z as usize).unwrap())
      } else {
        None
      }
    })
  }

  /// Gets the surrounding blocks, or none if they are out of bounds.
  pub fn get_surrounding_blocks_of(&self, x: i32, y: i32, z: i32) -> [Option<Block>; 6] {
    let block_array_lock = self.blocks.read().unwrap();
    let blocks = block_array_lock.as_ref();
    match blocks {
      Some(blocks) => {
        ADJACENT_OFFSETS.map(|[ox, oy, oz]| { //Map offsets.
          let (x_pos,y_pos,z_pos) = (ox + x, oy + y, oz + z);

          if CHUNK_RANGE_I32.contains(&x_pos) && CHUNK_RANGE_I32.contains(&y_pos) && CHUNK_RANGE_I32.contains(&z_pos) {
            Some(*blocks.get(x_pos as usize * CHUNK_SIZE * CHUNK_SIZE + y_pos as usize * CHUNK_SIZE + z_pos as usize).unwrap())
          } else {
            None
          }
        })
      },
      None => {
        [None; 6]
      },
    }
  }

  pub fn assign_if_waiting(&self) -> bool {
    let mut state = self.state.lock().unwrap();
    match &state.progress {
      ChunkStateProgress::Waiting => {
        state.progress = ChunkStateProgress::TaskAssigned;
        true
      },
      _ => false
    }
  }

  ///Generates the visibility for blocks. The adjacent chunks correspond to BlockSide for their direction.
  pub fn gen_block_vis(&self, adjacent_chunks: [Option<Arc<Chunk>>; 6]) {
    if !self.start_process_check(ChunkStateStage::ChunkVisGen) {
      return;
    }
    
    let block_read_lock = self.blocks.read().unwrap();
    let blocks = block_read_lock.as_ref().unwrap();
    let mut surface_visibility = Vec::<BlockSideVisibility>::with_capacity(blocks.len());
    for ((x, y, z), block_ref) in block_iterator().zip(blocks.iter()) {
      let block = *block_ref;
      if let Block::Air = block {
        surface_visibility.push(BlockSideVisibility::new(false));
        continue;
      }

      let surroundings = self.get_surrounding_blocks_of(x as i32, y as i32, z as i32);
      let mut vis = BlockSideVisibility::new(false);

      for (index, block) in surroundings.into_iter().enumerate() { //Iterate each surrounding block.
        let side = BlockSide::try_from(index as u8).unwrap(); //Convert the index to BlockSide.
        match block {
            Some(block) => vis.set_visible(side, block.is_translucent()), //Block is within chunk.
            None => { //Adjacent block is outisde chunk.
              // Chunk relative position.
              let rel_pos = match side {
                BlockSide::Right => [1, y, z],
                BlockSide::Left => [CHUNK_SIZE - 1, y, z],
                BlockSide::Above => [x, 1, z],
                BlockSide::Below => [x, CHUNK_SIZE - 1, z],
                BlockSide::Back => [x, y, 1],
                BlockSide::Front => [x, y, CHUNK_SIZE - 1],
              };
              
              let block: Option<Block> = adjacent_chunks.get(index).unwrap().as_ref().and_then(|chunk| 
                chunk.get_block_at(rel_pos[0] as i32, rel_pos[1] as i32, rel_pos[2] as i32)
              );

              let translucent = match block {
                Some(block) => block.is_translucent(),
                None => false,
              };

              vis.set_visible(side, translucent);
            },
        }
      }
      surface_visibility.push(vis);
    }
    *self.block_vis.lock().unwrap() = Some(surface_visibility);
    self.end_process_check(ChunkStateStage::ChunkVisGen, ChunkStateStage::MeshGen, || {
      
    });
  }

  /// Update the vertex buffer. gen_block_vis must be called at least once before this is called. This should only be called if the vertex state is outdated.
  pub fn update_vertices(&self, device: &Device, queue: &Queue) { //Generate a vertex buffer for the chunk.
    if !self.start_process_check(ChunkStateStage::MeshGen) {
      return;
    }
    let block_vis_lock = self.block_vis.lock().unwrap();
    let block_vis = block_vis_lock.as_ref().expect("Please call gen_block_vis before generating vertices.");
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let chunk_pos = self.chunk_id.map(|val| val * CHUNK_SIZE as i32);
    
    const WINDING_ORDER: [u32; 6] = [0, 1, 2, 2, 3, 0];

    for ((x, y, z), (block, block_visibility)) in block_iterator().zip(self.blocks.read().unwrap().as_ref().unwrap().iter().zip(block_vis)) {
      if block_visibility.is_invisible() {continue}; //Skip invisible blocks.
      let colour = block.get_colour();

      for side_i in 0..6 { //Corresponds to BlockSide values.
        let side = BlockSide::try_from(side_i).unwrap();

        if !block_visibility.get_visible(side) {continue}; //Skip this side if it is not visible.

        let normal = side.get_face_normal(); //get the face normal.
        let starting_index = vertices.len() as u32;
        
        for winding_index in WINDING_ORDER {
          let index = starting_index + winding_index;
          indices.push(index);
        }

        side.get_face_offset_vectors().iter().for_each(|vec| {

          let v_pos = [vec[0] + x as f32, vec[1] + y as f32, vec[2] + z as f32];
          vertices.push(ChunkVertex {
            absolute_position: chunk_pos.clone(),
            relative_position: v_pos,
            colour,
            normal
          });
        });
      }
    }
    self.update_vertex_buffer(device, queue, vertices, indices);
    self.end_process_check(ChunkStateStage::MeshGen, ChunkStateStage::Ready, || {
      //update vertex buffer here instead???
    });

  }

  fn update_vertex_buffer(&self, device: &Device, queue: &Queue, vertices: Vec<ChunkVertex>, indices: Vec<u32>) {
    let mut mesh_lock = self.mesh.lock().unwrap();
    match mesh_lock.as_mut() {
      Some(mesh) => {
        mesh.vertex_buffer.update(device, queue, &vertices);
        mesh.index_buffer.update(device, queue, &indices);
      },
      None => {
        *mesh_lock = Some(
          ChunkMesh {
              vertex_buffer: GenericBuffer::new(device, queue, GenericBufferType::Vertex, &vertices, 400),
              index_buffer: GenericBuffer::new(device, queue, GenericBufferType::Index, &indices, 600),
          }
        );
      },
    }
  }

  //Returns the vertex and index buffer unless they are being updated.
  pub fn get_mesh_fast(&self) -> Option<ChunkMeshData> {
    self.mesh.try_lock().ok()?.as_ref().map(|mesh| {
      ChunkMeshData {
        vertex_buffer: (mesh.vertex_buffer.get_buffer(), mesh.vertex_buffer.len() as u64),
        index_buffer: (mesh.index_buffer.get_buffer(), mesh.index_buffer.len() as u64),
      }
    }) 
  }

  pub fn get_id(&self) -> [i32; 3] {
    self.chunk_id.clone()
  }

  pub fn get_pending_stage(&self) -> Option<ChunkStateStage> {
    let state_lock = self.state.lock().unwrap();
    match &state_lock.progress {
      ChunkStateProgress::Waiting => {
        Some(state_lock.stage)
      },
      _ => None
    }
  }

  pub fn get_stage(&self) -> ChunkStateStage {
    self.state.lock().unwrap().stage
  }
}

// impl Drop for Chunk {
//   fn drop(&mut self) {
//     let current_thread = std::thread::current();
//     let thread_name = current_thread.name();
//     if let Some(name) = thread_name {
//       if name != "GC Thread" {
//         println!("Chunk dropped on thread: {}", name);
//       }
//     } else {
//       println!("Chunk dropped on unnamed thread.");
//     }
//   }
// }

fn block_iterator() -> impl Iterator<Item = (usize, usize, usize)> {
  iproduct!(CHUNK_RANGE, CHUNK_RANGE, CHUNK_RANGE)
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ChunkVertex {
  absolute_position: [i32; 3],
  relative_position: [f32; 3],
  colour: [f32; 3],
  normal: [f32; 3]
}