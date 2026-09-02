use crate::tagops::db::DynResult;
use edms::EdmsCore;
use edms::query_loader::QueryMap;

pub fn add_collection_tag(core: &EdmsCore, queries: &QueryMap, collection_name: &str, tagname: &str) -> DynResult<()> {
    let q = queries.get_merge_query("CT_INSERT").ok_or("missing CT_INSERT")?;
    core.proc(q, &[&collection_name, &tagname])?;
    Ok(())
}

pub fn remove_collection_tag(core: &EdmsCore, queries: &QueryMap, collection_name: &str, tagname: &str) -> DynResult<()> {
    let q = queries.get_merge_query("CT_DELETE").ok_or("missing CT_DELETE")?;
    core.proc(q, &[&collection_name, &tagname])?;
    Ok(())
}

pub fn list_collection_tags(core: &EdmsCore, queries: &QueryMap, collection_name: &str) -> DynResult<Vec<String>> {
    let q = queries.get_merge_query("CT_LIST").ok_or("missing CT_LIST")?;
    Ok(core.cproc(q, &[&collection_name], |row| row.get(0))?)
}

pub fn collections_with_tag(core: &EdmsCore, queries: &QueryMap, tagname: &str) -> DynResult<Vec<String>> {
    let q = queries.get_merge_query("CT_COLLECTIONS_BY_TAG").ok_or("missing CT_COLLECTIONS_BY_TAG")?;
    Ok(core.cproc(q, &[&tagname], |row| row.get(0))?)
}

pub fn merge_collection_tags_op(core: &EdmsCore, queries: &QueryMap, source_name: &str, target_name: &str) -> DynResult<usize> {
    let q = queries.get_merge_query("CT_MERGE").ok_or("missing CT_MERGE")?;
    Ok(core.proc(q, &[&target_name, &source_name])?)
}
