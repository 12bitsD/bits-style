use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::analyze::ImportRecord;
use crate::model::Violation;

#[derive(Debug, Clone, Deserialize)]
pub struct LayerRuleConfig {
    pub name: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub pure: bool,
    #[serde(default)]
    pub forbid_imports: Vec<String>,
    #[serde(default)]
    pub allow_imports: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct LayerPolicy {
    layers: Vec<CompiledLayer>,
}

#[derive(Debug)]
struct CompiledLayer {
    name: String,
    paths: GlobSet,
    pure: bool,
    forbid_imports: Vec<String>,
    forbid_globs: GlobSet,
    allow_imports: Option<Vec<String>>,
}

struct ImportContext<'a> {
    file: &'a str,
    import: &'a ImportRecord,
    resolved: Option<&'a str>,
    target_layer: Option<&'a str>,
}

impl LayerPolicy {
    pub fn compile(configs: &[LayerRuleConfig]) -> Result<Self, String> {
        let mut layers = Vec::with_capacity(configs.len());
        for config in configs {
            if config.name.trim().is_empty() {
                return Err(
                    "layer name in bits.toml must not be empty".to_owned()
                );
            }
            if config.paths.is_empty() {
                return Err(format!(
                    "layer '{}' in bits.toml must specify at least one path glob",
                    config.name
                ));
            }
            let paths = compile_globs(&config.paths)?;
            let forbid_globs = compile_globs(&config.forbid_imports)?;
            layers.push(CompiledLayer {
                name: config.name.clone(),
                paths,
                pure: config.pure,
                forbid_imports: config.forbid_imports.clone(),
                forbid_globs,
                allow_imports: config.allow_imports.clone(),
            });
        }
        Ok(Self { layers })
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn is_pure_file(&self, relative: &str) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.pure && layer.paths.is_match(relative))
    }

    pub fn layer_of(&self, relative: &str) -> Option<&str> {
        self.layers
            .iter()
            .find(|layer| layer.paths.is_match(relative))
            .map(|layer| layer.name.as_str())
    }

    pub fn check_imports(
        &self,
        file: &str,
        imports: &[ImportRecord],
        resolve_target: impl Fn(&ImportRecord) -> Option<String>,
        violations: &mut Vec<Violation>,
    ) {
        let matching = self.matching_layers(file);
        if matching.is_empty() {
            return;
        }
        for import in imports.iter().filter(|item| !item.type_only) {
            let resolved = resolve_target(import);
            let target = resolved.as_deref().and_then(|t| self.layer_of(t));
            let ctx = ImportContext {
                file,
                import,
                resolved: resolved.as_deref(),
                target_layer: target,
            };
            emit_layer_violations(&matching, &ctx, violations);
        }
    }

    fn matching_layers(&self, file: &str) -> Vec<&CompiledLayer> {
        self.layers
            .iter()
            .filter(|layer| layer.paths.is_match(file))
            .collect()
    }
}

fn emit_layer_violations(
    layers: &[&CompiledLayer],
    ctx: &ImportContext<'_>,
    violations: &mut Vec<Violation>,
) {
    for layer in layers {
        let Some(reason) = forbidden_reason(layer, ctx) else {
            continue;
        };
        let mut violation = Violation::new(
            "BCS017",
            ctx.file,
            ctx.import.line,
            ctx.import.column,
            reason,
            ctx.import.specifier.as_str(),
            format!("respect_layer_boundary({})", layer.name),
            false,
        );
        violation.mark_high_confidence_violation();
        violation.category = Some("role".to_owned());
        violation.prescription = Some(format!(
            "move this dependency out of layer '{}' or invert it through an explicit interface parameter",
            layer.name
        ));
        violations.push(violation);
    }
}

fn forbidden_reason(
    layer: &CompiledLayer,
    ctx: &ImportContext<'_>,
) -> Option<String> {
    if let Some(target_name) = ctx.target_layer {
        if layer.forbid_imports.iter().any(|item| item == target_name) {
            return Some(format!(
                "layer '{}' must not import from forbidden layer '{}'",
                layer.name, target_name
            ));
        }
        if let Some(allowed) = &layer.allow_imports
            && target_name != layer.name
            && !allowed.iter().any(|item| item == target_name)
        {
            return Some(format!(
                "layer '{}' may only import from {:?}, not '{}'",
                layer.name, allowed, target_name
            ));
        }
    }
    if layer.forbid_globs.is_match(&ctx.import.specifier)
        || ctx.resolved.is_some_and(|p| layer.forbid_globs.is_match(p))
    {
        return Some(format!(
            "layer '{}' must not import forbidden dependency '{}'",
            layer.name, ctx.import.specifier
        ));
    }
    None
}

fn compile_globs(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| error.to_string())?);
    }
    builder.build().map_err(|error| error.to_string())
}
