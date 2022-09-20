# Bevy Into Render Asset

Prepare assets of type `T` into render assets of type `U`. Where type `U` is the prepared render asset of another Type `X`.

In other words: You can for example prepare your mesh-like asset into `GpuMesh`.

```rust
impl IntoRenderAsset for GpuGeneratedMesh {
    type ExtractedAsset = GpuGeneratedMesh;
    type Into = Mesh;
    type Param = SRes<RenderDevice>;

    fn extract_asset(&self) -> Self::ExtractedAsset {
        self.clone()
    }

    fn prepare_asset_into(
        gpu_generated_mesh: Self::ExtractedAsset,
        render_device: &mut SystemParamItem<Self::Param>,
    ) -> Result<<Self::Into as RenderAsset>::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>>
    {
        let vertex_buffer = gpu_generated_mesh.vertex_buffer;

        let buffer_info = GpuBufferInfo::Indexed {
            buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                usage: BufferUsages::INDEX,
                contents: gpu_generated_mesh.get_index_buffer_bytes(),
                label: Some("gpu generated mesh index buffer"),
            }),
            count: gpu_generated_mesh.indices().unwrap().len() as u32,
            index_format: gpu_generated_mesh.indices().unwrap().into(),
        };

        let mesh_vertex_buffer_layout = gpu_generated_mesh.get_mesh_vertex_buffer_layout();

        Ok(GpuMesh {
            vertex_buffer,
            buffer_info,
            primitive_topology: gpu_generated_mesh.primitive_topology(),
            layout: mesh_vertex_buffer_layout,
        })
    }
}
```

```rust
app.add_plugin(IntoRenderAssetPlugin::<GpuGeneratedMesh>::default());
```

Note: Dont forget to extract the handles of your assets.

You can't access the render assets of type `U` with handles for assets of type `T`. But you can map handles for assets of type `T` to `X` with [`bevy-map-handle`](https://github.com/VVishion/bevy-map-handle) which `IntoRenderAssetPlugin` also uses internally.

```rust
pub fn extract_gpu_generated_mesh_handles(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    query: Extract<Query<(Entity, &Handle<GpuGeneratedMesh>)>>,
) {
    let mut handles = Vec::with_capacity(*previous_len);
    for (entity, handle) in query.iter() {
        let mapped = match handle.map_weak::<<GpuGeneratedMesh as IntoRenderAsset>::Into>() {
            Err(_) => continue,
            Ok(handle) => handle,
        };

        handles.push((entity, (mapped, handle)));
    }
    *previous_len = handles.len();
    commands.insert_or_spawn_batch(handles);
}
```