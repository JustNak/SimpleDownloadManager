use super::{
    resolution_error, FailedHosterLink, HosterResolutionBatch, HosterResolutionError,
    ResolvedHosterLink,
};

type IndexedResolvedHosterLink = Result<(usize, ResolvedHosterLink), (usize, FailedHosterLink)>;

#[cfg(test)]
pub(super) fn ordered_resolved_hoster_links(
    indexed: Vec<IndexedResolvedHosterLink>,
    expected_len: usize,
) -> Result<Vec<ResolvedHosterLink>, HosterResolutionError> {
    let batch = ordered_resolved_hoster_batch(indexed, expected_len)?;
    if let Some(failed) = batch.failed_links.into_iter().next() {
        return Err(resolution_error(failed.message));
    }
    Ok(batch.links)
}

pub(super) fn ordered_resolved_hoster_batch(
    mut indexed: Vec<IndexedResolvedHosterLink>,
    expected_len: usize,
) -> Result<HosterResolutionBatch, HosterResolutionError> {
    indexed.sort_by_key(|result| match result {
        Ok((index, _)) | Err((index, _)) => *index,
    });

    if indexed.len() != expected_len {
        return Err(resolution_error(
            "Hoster resolver returned incomplete results.".into(),
        ));
    }

    let mut links = Vec::with_capacity(expected_len);
    let mut failed_links = Vec::new();
    for (expected_index, result) in indexed.into_iter().enumerate() {
        match result {
            Ok((index, link)) if index == expected_index => links.push(link),
            Err((index, failed)) if index == expected_index => failed_links.push(failed),
            Ok(_) | Err(_) => {
                return Err(resolution_error(
                    "Hoster resolver returned results with inconsistent indexes.".into(),
                ));
            }
        }
    }

    Ok(HosterResolutionBatch {
        links,
        failed_links,
    })
}
