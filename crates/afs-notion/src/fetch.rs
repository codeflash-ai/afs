//! Fetch full Notion page content from the paginated block API.

use afs_core::AfsResult;

use crate::client::NotionApi;
use crate::dto::{BlockTreeDto, NotionPageBundle};

pub fn fetch_page_bundle(api: &dyn NotionApi, page_id: &str) -> AfsResult<NotionPageBundle> {
    let page = api.retrieve_page(page_id)?;
    let blocks = fetch_block_trees(api, page_id)?;

    Ok(NotionPageBundle { page, blocks })
}

fn fetch_block_trees(api: &dyn NotionApi, block_id: &str) -> AfsResult<Vec<BlockTreeDto>> {
    let mut cursor = None;
    let mut trees = Vec::new();

    loop {
        let page = api.retrieve_block_children(block_id, cursor.as_deref())?;
        for block in page.results {
            let children = if block.has_children {
                fetch_block_trees(api, &block.id)?
            } else {
                Vec::new()
            };
            trees.push(BlockTreeDto { block, children });
        }

        if !page.has_more {
            break;
        }
        cursor = page.next_cursor;
    }

    Ok(trees)
}
