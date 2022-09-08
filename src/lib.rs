use bevy::app::{App, Plugin};
use bevy::asset::{Asset, AssetEvent, Assets, Handle};
use bevy::ecs::{
    prelude::*,
    system::{StaticSystemParam, SystemParam, SystemParamItem},
};
use bevy::render::render_asset::{PrepareAssetError, PrepareAssetLabel, RenderAsset};
use bevy::render::{Extract, RenderApp, RenderStage};
use bevy::utils::{HashMap, HashSet};
use bevy_map_handle::MapHandle;
use std::marker::PhantomData;

/// Describes how an asset gets extracted and prepared for rendering into [`RenderAsset::PreparedAsset`] of an existing `RenderAsset` specified by [`IntoRenderAsset::Into`].
///
/// In the [`RenderStage::Extract`](crate::RenderStage::Extract) step the asset is transferred from the `MainWorld` into the `RenderWorld`.
/// It is converted into [`IntoRenderAsset::ExtractedAsset`] in the process.
/// `IntoRenderAsset::ExtractedAsset` is often `T` for `T` implementing `IntoRenderAsset`.
///
/// In the following [`RenderStage::Prepare`](crate::RenderStage::Prepare) step the extracted asset is transformed into the GPU-representation of type [`RenderAsset::PreparedAsset`] of an existing `RenderAsset` specified by [`IntoRenderAsset::Into`].
pub trait IntoRenderAsset: Asset {
    /// The representation of the asset in the `RenderWorld`.
    type ExtractedAsset: Send + Sync + 'static;
    /// The [`RenderAsset`] which [`RenderAsset::PreparedAsset`] this asset will be prepared into.
    type Into: RenderAsset;
    /// Specifies all ECS data required by [`IntoRenderAsset::prepare_asset_into`].
    /// For convenience use the [`lifetimeless`](bevy::ecs::system::lifetimeless) [`SystemParam`].
    type Param: SystemParam;
    /// Transforms this asset into [`IntoRenderAsset::ExtractedAsset`].
    fn extract_asset(&self) -> Self::ExtractedAsset;
    /// Prepares [`IntoRenderAsset::ExtractedAsset`] for the GPU by transforming it into [`RenderAsset::PreparedAsset`] of [`IntoRenderAsset::Into`].
    /// Therefore ECS data may be accessed via the `param`.
    fn prepare_asset_into(
        extracted_asset: Self::ExtractedAsset,
        param: &mut SystemParamItem<Self::Param>,
    ) -> Result<<Self::Into as RenderAsset>::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>>;
}

/// This plugin extracts the changed assets from the `MainWorld` of type `T` and prepares them in the `RenderWorld` into [`RenderAsset::PreparedAsset`] of type `U`.
/// They can be accessed from [`bevy::render::render_asset::RenderAssets<U>`] or [`IntoRenderAssets<T>`].
///
/// It therefore sets up the [`RenderStage::Extract`](crate::RenderStage::Extract) and
/// [`RenderStage::Prepare`](crate::RenderStage::Prepare) steps for the specified [`IntoRenderAsset`].
pub struct IntoRenderAssetPlugin<A: IntoRenderAsset> {
    prepare_asset_label: PrepareAssetLabel,
    phantom: PhantomData<fn() -> A>,
}

impl<A: IntoRenderAsset> IntoRenderAssetPlugin<A> {
    pub fn with_prepare_asset_label(prepare_asset_label: PrepareAssetLabel) -> Self {
        Self {
            prepare_asset_label,
            phantom: PhantomData,
        }
    }
}

impl<A: IntoRenderAsset> Default for IntoRenderAssetPlugin<A> {
    fn default() -> Self {
        Self {
            prepare_asset_label: Default::default(),
            phantom: PhantomData,
        }
    }
}

impl<A: IntoRenderAsset> Plugin for IntoRenderAssetPlugin<A> {
    fn build(&self, app: &mut App) {
        if let Ok(render_app) = app.get_sub_app_mut(RenderApp) {
            let prepare_asset_system = prepare_assets::<A>.label(self.prepare_asset_label.clone());

            let prepare_asset_system = match self.prepare_asset_label {
                PrepareAssetLabel::PreAssetPrepare => prepare_asset_system,
                PrepareAssetLabel::AssetPrepare => {
                    prepare_asset_system.after(PrepareAssetLabel::PreAssetPrepare)
                }
                PrepareAssetLabel::PostAssetPrepare => {
                    prepare_asset_system.after(PrepareAssetLabel::AssetPrepare)
                }
            };

            render_app
                .init_resource::<ExtractedAssets<A>>()
                .init_resource::<IntoRenderAssets<A>>()
                .init_resource::<PrepareNextFrameAssets<A>>()
                .add_system_to_stage(RenderStage::Extract, extract_render_asset::<A>)
                .add_system_to_stage(RenderStage::Prepare, prepare_asset_system);
        }
    }
}

/// Temporarily stores the extracted and removed assets of the current frame.
pub struct ExtractedAssets<A: IntoRenderAsset> {
    extracted: Vec<(Handle<A>, A::ExtractedAsset)>,
    removed: Vec<Handle<A>>,
}

impl<A: IntoRenderAsset> Default for ExtractedAssets<A> {
    fn default() -> Self {
        Self {
            extracted: Default::default(),
            removed: Default::default(),
        }
    }
}

/// Stores all GPU representations ([`RenderAsset::PreparedAsset`] of [`IntoRenderAsset::Into`]) of type `T` implementing [`IntoRenderAsset`]
pub type IntoRenderAssets<A> = HashMap<
    Handle<<A as IntoRenderAsset>::Into>,
    <<A as IntoRenderAsset>::Into as RenderAsset>::PreparedAsset,
>;

/// This system extracts created or modified assets into the `RenderWorld`.
fn extract_render_asset<A: IntoRenderAsset>(
    mut commands: Commands,
    mut events: Extract<EventReader<AssetEvent<A>>>,
    assets: Extract<Res<Assets<A>>>,
) {
    let mut changed_assets = HashSet::default();
    let mut removed = Vec::new();
    for event in events.iter() {
        match event {
            AssetEvent::Created { handle } | AssetEvent::Modified { handle } => {
                changed_assets.insert(handle.clone_weak());
            }
            AssetEvent::Removed { handle } => {
                changed_assets.remove(&handle);
                removed.push(handle.clone_weak());
            }
        }
    }

    let mut extracted_assets = Vec::new();
    for handle in changed_assets.drain() {
        if let Some(asset) = assets.get(&handle) {
            extracted_assets.push((handle, asset.extract_asset()));
        }
    }

    commands.insert_resource(ExtractedAssets {
        extracted: extracted_assets,
        removed,
    });
}

/// Assets queued to be prepared next frame.
pub struct PrepareNextFrameAssets<A: IntoRenderAsset> {
    assets: Vec<(Handle<A>, A::ExtractedAsset)>,
}

impl<A: IntoRenderAsset> Default for PrepareNextFrameAssets<A> {
    fn default() -> Self {
        Self {
            assets: Default::default(),
        }
    }
}

/// This system prepares [`IntoRenderAsset`] assets into [`RenderAsset::PreparedAsset`] of [`IntoRenderAsset::Into`] if extracted this frame or failed to prepare previously.
fn prepare_assets<R: IntoRenderAsset>(
    mut extracted_assets: ResMut<ExtractedAssets<R>>,
    mut render_assets: ResMut<IntoRenderAssets<R>>,
    mut prepare_next_frame: ResMut<PrepareNextFrameAssets<R>>,
    param: StaticSystemParam<<R as IntoRenderAsset>::Param>,
) {
    let mut param = param.into_inner();
    let mut queued_assets = std::mem::take(&mut prepare_next_frame.assets);
    for (handle, extracted_asset) in queued_assets.drain(..) {
        match R::prepare_asset_into(extracted_asset, &mut param) {
            Ok(prepared_asset) => {
                let handle = match handle.map_weak() {
                    Err(_) => panic!("Shouldn't be preparing pending assets."),
                    Ok(handle) => handle,
                };

                render_assets.insert(handle, prepared_asset);
            }
            Err(PrepareAssetError::RetryNextUpdate(extracted_asset)) => {
                prepare_next_frame.assets.push((handle, extracted_asset));
            }
        }
    }

    for removed in std::mem::take(&mut extracted_assets.removed) {
        let handle = match removed.map_weak() {
            Err(_) => panic!("Shouldn't be removing pending assets."),
            Ok(handle) => handle,
        };

        render_assets.remove(&handle);
    }

    for (handle, extracted_asset) in std::mem::take(&mut extracted_assets.extracted) {
        match R::prepare_asset_into(extracted_asset, &mut param) {
            Ok(prepared_asset) => {
                let handle = match handle.map_weak() {
                    Err(_) => panic!("Shouldn't be preparing pending assets."),
                    Ok(handle) => handle,
                };

                render_assets.insert(handle, prepared_asset);
            }
            Err(PrepareAssetError::RetryNextUpdate(extracted_asset)) => {
                prepare_next_frame.assets.push((handle, extracted_asset));
            }
        }
    }
}
