use afs_connector::{Connector, FetchRequest};
use afs_core::AfsResult;
use afs_notion::NotionConnector;
use afs_notion::dto::NotionPageBundle;

use crate::hydration::{HydratedEntity, HydrationSource};

impl HydrationSource for NotionConnector {
    fn fetch_render(
        &self,
        request: &afs_core::hydration::HydrationRequest,
    ) -> AfsResult<HydratedEntity> {
        let native = self.fetch(FetchRequest {
            remote_id: request.remote_id.clone(),
        })?;
        let rendered = self.render_native_entity(&native)?;
        let bundle = serde_json::from_slice::<NotionPageBundle>(&native.raw).map_err(|error| {
            afs_core::AfsError::Io(format!("notion native decode failed: {error}"))
        })?;

        Ok(HydratedEntity {
            document: rendered.document,
            shadow: rendered.shadow,
            remote_edited_at: bundle.page.last_edited_time,
        })
    }
}
