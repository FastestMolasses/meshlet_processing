# Bevy Meshlet Converter
A simple command-line utility for converting GLTF/GLB meshes into Bevy's experimental meshlet format.

## Usage

1. Create a directory called meshlet_source in your project root
2. Place your GLTF/GLB files in the meshlet_source directory
3. Run the script
4. Converted meshlet files will be output to a meshlets directory

# Requirements

- The input meshes must have POSITION, NORMAL, and UV_0 attributes
- When exporting from Blender, ensure these settings are enabled and everything else is disabled:
    - UVs
    - Normals
    - Apply Modifiers (if using any)
