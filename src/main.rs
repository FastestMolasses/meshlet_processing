use bevy::{
    asset::{
        AsyncWriteExt, ErasedLoadedAsset, LoadedAsset, io::AssetSourceId, saver::ErasedAssetSaver,
    },
    gltf::{Gltf, GltfMesh},
    pbr::experimental::meshlet::{MeshletMesh, MeshletMeshSaver},
    prelude::*,
    render::mesh::Mesh,
    tasks::AsyncComputeTaskPool,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

#[derive(Resource)]
struct MeshProcessingConfig {
    input_dir: PathBuf,
    output_dir: PathBuf,
}

#[derive(Resource, Default)]
struct MeshProcessingState {
    pending_files: Vec<(PathBuf, Handle<Gltf>)>,
    processed_files: Vec<PathBuf>,
    active_tasks: Arc<AtomicUsize>,
}

fn main() {
    let project_root = std::env::current_dir().expect("Failed to get current directory");

    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(MeshProcessingConfig {
            input_dir: project_root.join("meshlet_source"),
            output_dir: project_root.join("meshlets"),
        })
        .add_systems(Startup, setup_mesh_processing)
        .add_systems(Update, process_loaded_meshes)
        .run();
}

fn queue_files_recursive(
    dir: &PathBuf,
    asset_server: &AssetServer,
    pending_files: &mut Vec<(PathBuf, Handle<Gltf>)>,
) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue_files_recursive(&path, asset_server, pending_files);
            } else if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "gltf" || ext == "glb" {
                    println!(
                        "{}Queueing {} for processing{}",
                        CYAN,
                        path.file_name()
                            .and_then(|file_name| file_name.to_str())
                            .unwrap_or("unknown"),
                        RESET
                    );
                    let handle = asset_server.load(path.to_str().unwrap());
                    pending_files.push((path, handle));
                }
            }
        }
    }
}

fn setup_mesh_processing(
    mut commands: Commands,
    config: Res<MeshProcessingConfig>,
    asset_server: Res<AssetServer>,
) {
    fs::create_dir_all(&config.output_dir).expect("Failed to create output directory");
    let mut state = MeshProcessingState {
        active_tasks: Arc::new(AtomicUsize::new(0)),
        ..Default::default()
    };
    queue_files_recursive(&config.input_dir, &asset_server, &mut state.pending_files);
    commands.insert_resource(state);
}

fn process_loaded_meshes(
    mut state: ResMut<MeshProcessingState>,
    config: Res<MeshProcessingConfig>,
    asset_server: Res<AssetServer>,
    gltf_assets: Res<Assets<Gltf>>,
    gltf_mesh_assets: Res<Assets<GltfMesh>>,
    mesh_assets: Res<Assets<Mesh>>,
) {
    let mut completed_indices = Vec::new();

    for (index, (path, handle)) in state.pending_files.iter().enumerate() {
        if !asset_server.is_loaded_with_dependencies(handle) {
            continue;
        }

        if let Some(gltf) = gltf_assets.get(handle) {
            for mesh in gltf.meshes.iter() {
                println!(
                    "{}Processing mesh {}{}",
                    YELLOW,
                    mesh.path()
                        .and_then(|asset_path| asset_path.path().file_name())
                        .and_then(|file_name| file_name.to_str())
                        .unwrap_or("unknown"),
                    RESET
                );
                if let Some(gltf_mesh) = gltf_mesh_assets.get(mesh) {
                    for primitive in gltf_mesh.primitives.iter() {
                        if let Some(mesh) = mesh_assets.get(&primitive.mesh) {
                            let mesh = mesh
                                .clone()
                                .with_removed_attribute(Mesh::ATTRIBUTE_TANGENT)
                                .with_removed_attribute(Mesh::ATTRIBUTE_COLOR)
                                .with_removed_attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
                                .with_removed_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
                                .with_removed_attribute(Mesh::ATTRIBUTE_UV_1);
                            let meshlet = MeshletMesh::from_mesh(&mesh, 1);

                            let stem = path.file_stem().unwrap();
                            let output_path = config
                                .output_dir
                                .join(format!("{}.meshlet", stem.to_string_lossy()));

                            match meshlet {
                                Ok(meshlet) => {
                                    let active_tasks = Arc::clone(&state.active_tasks);
                                    let asset_server = asset_server.clone();
                                    let output_path_clone = output_path.clone();

                                    active_tasks.fetch_add(1, Ordering::SeqCst);

                                    let task_pool = AsyncComputeTaskPool::get();
                                    task_pool
                                        .spawn(async move {
                                            let source = asset_server
                                                .get_source(AssetSourceId::Default)
                                                .unwrap();
                                            let writer = source.writer().unwrap();

                                            if let Ok(mut write) = writer.write(&output_path_clone).await {
                                                let saver = MeshletMeshSaver;
                                                let loaded_asset = LoadedAsset::new_with_dependencies(
                                                    meshlet,
                                                    None,
                                                );
                                                let erased = ErasedLoadedAsset::from(loaded_asset);

                                                match saver.save(&mut *write, &erased, &()).await {
                                                    Ok(_) => {
                                                        if let Err(e) = write.flush().await {
                                                            eprintln!("Failed to flush write: {}", e);
                                                        } else {
                                                            println!(
                                                                "{}Successfully saved meshlet to {:?}{}",
                                                                GREEN,
                                                                output_path_clone,
                                                                RESET
                                                            );
                                                        }
                                                    }
                                                    Err(e) => eprintln!("Failed to save meshlet: {}", e),
                                                }
                                            }
                                            active_tasks.fetch_sub(1, Ordering::SeqCst);
                                        })
                                        .detach();
                                }
                                Err(e) => eprintln!(
                                    "{}Failed to convert mesh to meshlet: {}{}",
                                    RED, e, RESET
                                ),
                            }
                        }
                    }
                }
            }
            completed_indices.push(index);
        }
    }

    // Remove processed files
    for &index in completed_indices.iter().rev() {
        let (path, _) = state.pending_files.remove(index);
        state.processed_files.push(path);
    }

    // Exit when all files are processed and all tasks are complete
    if state.pending_files.is_empty() && state.active_tasks.load(Ordering::SeqCst) == 0 {
        println!("\n{}All processing complete{}", GREEN, RESET);
        std::process::exit(0);
    }
}
