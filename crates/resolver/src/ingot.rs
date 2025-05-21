use std::collections::HashMap;

use camino::Utf8PathBuf;
use common::config::{Config, Dependency, DependencyDescription, IngotArguments};
use smol_str::SmolStr;

use crate::{files::FilesResolver, graph::GraphResolverImpl, ResolutionHandler, Resolver};

pub type IngotGraphResolver<NH> = GraphResolverImpl<FilesResolver, NH, (SmolStr, IngotArguments)>;

pub fn basic_ingot_graph_resolver() -> IngotGraphResolver<BasicIngotNodeHandler> {
    GraphResolverImpl::new(
        FilesResolver::exact_file("fe.toml".into()),
        BasicIngotNodeHandler::default(),
    )
}

pub fn ingot_graph_resolver<NH>(node_handler: NH) -> IngotGraphResolver<NH> {
    let files_resolver = FilesResolver::with_patterns(&["fe.toml", "src/**/*.fe"]);
    GraphResolverImpl::new(files_resolver, node_handler)
}

#[derive(Debug)]
pub struct IngotConfigDoesNotExist;

#[derive(Debug)]
pub struct UnresolvedDependency;

pub type BasicIngotGraphResolver = IngotGraphResolver<BasicIngotNodeHandler>;

#[derive(Default)]
pub struct BasicIngotNodeHandler {
    pub configs: HashMap<Utf8PathBuf, Config>,
}

impl ResolutionHandler<FilesResolver> for BasicIngotNodeHandler {
    type Item = Vec<(Utf8PathBuf, (SmolStr, IngotArguments))>;

    fn handle_resolution(
        &mut self,
        ingot_path: &Utf8PathBuf,
        mut files: Vec<(Utf8PathBuf, String)>,
    ) -> Self::Item {
        if let Some((_file_path, content)) = files.pop() {
            let config = Config::from_string(content);
            self.configs.insert(ingot_path.clone(), config.clone());
            return config
                .dependencies
                .into_iter()
                .map(
                    |Dependency {
                         alias,
                         description: DependencyDescription { url, arguments },
                     }| {
                        (
                            Utf8PathBuf::from_path_buf(
                                url.to_file_path().expect("url should be a file path"),
                            )
                            .expect("url should be a file path"),
                            (
                                alias,
                                arguments.unwrap_or_else(|| IngotArguments::default()),
                            ),
                        )
                    },
                )
                .collect();
        }

        return vec![];
    }
}
