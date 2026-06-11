use afs_connector::{Connector, FetchRequest};
use afs_core::AfsResult;
use afs_notion::NotionConnector;
use afs_notion::media::fetch_media_assets;

use crate::hydration::{HydratedAsset, HydratedEntity, HydrationSource};

impl HydrationSource for NotionConnector {
    fn fetch_render(
        &self,
        request: &afs_core::hydration::HydrationRequest,
    ) -> AfsResult<HydratedEntity> {
        let native = self.fetch(FetchRequest {
            remote_id: request.remote_id.clone(),
        })?;
        let rendered = self.render_native_entity_for_path(&native, &request.path)?;
        let assets = fetch_media_assets(&rendered.media_assets)?
            .into_iter()
            .map(|asset| HydratedAsset {
                path: asset.local_path,
                bytes: asset.bytes,
            })
            .collect();

        Ok(HydratedEntity {
            document: rendered.document,
            shadow: rendered.shadow,
            assets,
        })
    }
}
