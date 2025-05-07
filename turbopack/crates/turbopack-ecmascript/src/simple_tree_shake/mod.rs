//! Intermediate tree shaking that uses global information but not good as the full tree shaking.

use anyhow::{Context, Result};
use auto_hash_map::{AutoMap, AutoSet};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack_core::{module_graph::ModuleGraph, resolve::ExportUsage};

use crate::chunk::EcmascriptChunkPlaceable;

#[turbo_tasks::function]
pub async fn get_module_export_usages(
    graph: ResolvedVc<ModuleGraph>,
    module: ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>,
) -> Result<Vc<ModuleExportUsageInfo>> {
    let export_usage_info = compute_export_usage_info(graph)
        .resolve_strongly_consistent()
        .await?;

    let export_usage_info = export_usage_info.await?;

    let Some(exports) = export_usage_info.used_exports.get(&module) else {
        // We exclude template files from tree shaking because they are entrypoints to the module
        // graph.
        return Ok(ModuleExportUsageInfo::all());
    };

    Ok(**exports)
}

#[turbo_tasks::function(operation)]
async fn compute_export_usage_info(graph: ResolvedVc<ModuleGraph>) -> Result<Vc<ExportUsageInfo>> {
    let mut used_exports = AutoMap::<_, AutoSet<ExportUsage>>::default();

    graph
        .await?
        .traverse_all_edges_unordered(|(_, ref_data), target| {
            if let Some(target_module) =
                ResolvedVc::try_downcast::<Box<dyn EcmascriptChunkPlaceable>>(target.module)
            {
                used_exports
                    .entry(target_module)
                    .or_default()
                    .insert(ref_data.export.clone());
            }

            Ok(())
        })
        .await
        .context("failed to traverse module graph")?;

    let mut result = ExportUsageInfo::default();

    for (module, exports) in used_exports {
        result
            .used_exports
            .insert(module, ModuleExportUsageInfo { exports }.resolved_cell());
    }

    Ok(result.cell())
}

#[turbo_tasks::value]
#[derive(Default)]
pub struct ExportUsageInfo {
    used_exports:
        AutoMap<ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>, ResolvedVc<ModuleExportUsageInfo>>,
}

#[turbo_tasks::value]
pub struct ModuleExportUsageInfo {
    exports: AutoSet<ExportUsage>,
}

impl ModuleExportUsageInfo {
    pub fn is_export_used(&self, export_name: RcStr) -> bool {
        self.exports.contains(&ExportUsage::All)
            || self.exports.contains(&ExportUsage::Named(export_name))
    }
}

#[turbo_tasks::value_impl]
impl ModuleExportUsageInfo {
    #[turbo_tasks::function]
    pub fn all() -> Vc<Self> {
        let mut exports = AutoSet::with_capacity(1);
        exports.insert(ExportUsage::All);

        Self { exports }.cell()
    }
}
