/*
Copyright 2021 Volt Contributors
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at
    http://www.apache.org/licenses/LICENSE-2.0
Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

pub mod api;
pub mod http_manager;
pub mod package;

// Std Imports
use std::fs::File;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicI16};

// use rkyv::archived_root;
use anyhow::{anyhow, Result};
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use speedy::{Writable, Readable};
// use serde::{Deserialize, Serialize};



use indicatif::{ProgressBar, ProgressStyle};
use package::{Package, Version};
use tokio::{
    self,
    sync::{mpsc, Mutex},
};

use colored::Colorize;

use std::sync::atomic::Ordering;

#[derive(Clone)]
pub struct Main {
    pub dependencies: Arc<Mutex<Vec<(Package, Version)>>>,
    pub total_dependencies: Arc<AtomicI16>,
    pub sender: mpsc::Sender<()>,
}

#[derive(Debug, Clone, Writable, Readable)]
struct SpeedyVoltResponse {
    version: String,
    versions: HashMap<String, SpeedyVoltPackage>,
}

#[derive(Debug, Clone, Writable, Readable)]
struct SpeedyVoltPackage {
    pub integrity: String,
    pub tarball: String,
    pub bin: Option<HashMap<String, String>>,
    pub dependencies: Option<Vec<String>>,
    pub peer_dependencies: Option<Vec<String>>,
}

#[tokio::main]
async fn main() {
    let mut input_packages: Vec<String> = std::env::args().collect();

    input_packages.remove(0);
    let (tx, mut rx) = mpsc::channel(100);
    let add = Main::new(tx);

    {
        let packages = input_packages.clone();
        for package_name in packages {
            let mut add = add.clone();
            tokio::spawn(async move {
                add.get_dependency_tree(package_name.clone(), None)
                    .await
                    .ok();
            });
        }
    }

    let progress_bar = ProgressBar::new(1);

    progress_bar.set_style(
        ProgressStyle::default_bar()
            .progress_chars("=> ")
            .template(&format!(
                "{} [{{bar:40.magenta/green}}] {{msg:.blue}}",
                "Fetching dependencies".bright_blue()
            )),
    );

    let mut done: i16 = 0;

    while let Some(_) = rx.recv().await {
        done += 1;
        let total = add.total_dependencies.load(Ordering::Relaxed);
        if done == total {
            break;
        }
        progress_bar.set_length(total as u64);
        progress_bar.set_position(done as u64);
    }
    progress_bar.finish_with_message("[DONE]");

    println!(
        "Resolved {} dependencies.",
        add.dependencies
            .lock()
            .map(|deps| deps
                .iter()
                .map(|(dep, ver)| format!("{}: {}", dep.name, ver.version))
                .collect::<Vec<_>>()
                .len())
            .await
    );
    // TODO: Ensure that the total number of dependencies is more than 10, otherwise, it's usually
    // faster to resolve using the same algorithm as yarn and npm

    let dependencies = Arc::try_unwrap(add.dependencies).unwrap().into_inner();

    let mut version_data: HashMap<String, SpeedyVoltPackage> = HashMap::new();

    let mut parent_version: String = String::new();

    for dependency in dependencies.iter() {
        let mut deps: Option<Vec<String>> = Some(
            dependency
                .clone()
                .1
                .dependencies
                .into_iter()
                .map(|(k, _)| k)
                .collect(),
        );

        if deps.as_ref().unwrap().len() == 0 {
            deps = None;
        }

        let d1 = dependency.1.clone();

        let mut integrity = String::new();

        if d1.dist.integrity != String::new() {
            integrity = d1.clone().dist.integrity;
        } else {
            integrity = format!("sha1-{}", base64::encode(d1.clone().dist.tarball));
        }

        let mut pds: Option<Vec<String>> = Some(
            d1.clone()
                .peer_dependencies
                .into_iter()
                .map(|(k, _)| k)
                .collect(),
        );

        if pds.as_ref().unwrap().len() == 0 {
            pds = None;
        }

        #[allow(unused_assignments)]
        let package = SpeedyVoltPackage {
            peer_dependencies: pds,
            dependencies: deps,
            integrity: integrity,
            bin: None,
            tarball: d1.clone().dist.tarball,
        };

        if d1.clone().name == input_packages[0].clone() {
            parent_version  = d1.clone().name;
        }

        version_data.insert(
            format!("{}@{}", d1.clone().name, d1.clone().version),
            package,
        );
    }

    let res: SpeedyVoltResponse = SpeedyVoltResponse {
        version: parent_version,
        versions: version_data,
    };

    let bytes = res.write_to_vec().unwrap();

    let mut file = File::create(PathBuf::from("packages").join(format!("{}.sp", input_packages[0].clone()))).unwrap();

    file.write(&bytes).unwrap();

    // deser
    // drop(file);

    // let mut file = File::open(format!("packages\\{}.rkyv", input_packages[0].clone())).unwrap();

    // let start = std::time::Instant::now();

    // let mut bytes: Vec<u8> = vec![];

    // file.read_to_end(&mut bytes).unwrap();

    // let archived = unsafe { archived_root::<SpeedyVoltResponse>(&bytes[..]) };
    // let deserialized: SpeedyVoltResponse = SpeedyVoltResponse::read_from_buffer(&bytes).unwrap();

    // println!("Rkyv, deser: {}", start.elapsed().as_secs_f32());
}

impl Main {
    fn new(progress_sender: mpsc::Sender<()>) -> Self {
        Self {
            dependencies: Arc::new(Mutex::new(Vec::with_capacity(1))),
            total_dependencies: Arc::new(AtomicI16::new(0)),
            sender: progress_sender,
        }
    }

    async fn fetch_package(
        package_name: &str,
        version_range: Option<semver_rs::Range>,
    ) -> Result<(Package, Version)> {
        let package = http_manager::get_package(&package_name).await;

        let version: Version = match &version_range {
            Some(req) => {
                let mut available_versions: Vec<semver_rs::Version> = package
                    .versions
                    .iter()
                    .filter_map(|(k, _)| semver_rs::Version::new(k).parse().ok())
                    .collect();

                available_versions.sort_by(|v1, v2| {
                    semver_rs::compare(v1.to_string().as_str(), v2.to_string().as_str(), None)
                        .unwrap()
                });

                available_versions.reverse();

                available_versions
                    .into_iter()
                    .find(|v| req.test(v))
                    .map(|v| package.versions.get(&v.to_string()))
                    .flatten()
            }
            None => package.versions.get(&package.dist_tags.latest),
        }
        .ok_or_else(|| {
            if let Some(_) = version_range {
                anyhow!("Version for '{}' is not found", &package_name)
            } else {
                anyhow!("Unable to find latest version for '{}'", &package_name)
            }
        })?
        .clone();

        Ok((package, version))
    }

    fn get_dependency_tree(
        &mut self,
        package_name: String,
        version_req: Option<semver_rs::Range>,
    ) -> BoxFuture<'_, Result<()>> {
        async move {
            let pkg = Self::fetch_package(&package_name, version_req).await?;
            let pkg_deps = pkg.1.dependencies.clone();

            let should_download = self
                .dependencies
                .lock()
                .map(|mut deps| {
                    if !deps.iter().any(|(package, version)| {
                        package.name == pkg.0.name && pkg.1.version == version.version
                    }) {
                        deps.push(pkg);
                        true
                    } else {
                        false
                    }
                })
                .await;

            if !should_download {
                return Ok(());
            }

            let mut workers = FuturesUnordered::new();

            self.total_dependencies.store(
                self.total_dependencies.load(Ordering::Relaxed) + 1,
                Ordering::Relaxed,
            );

            for (name, version) in pkg_deps {
                let range = semver_rs::Range::new(&version).parse().unwrap();

                // Increase total
                self.total_dependencies.store(
                    self.total_dependencies.load(Ordering::Relaxed) + 1,
                    Ordering::Relaxed,
                );

                let pkg_name = name.clone();
                let mut self_copy = self.clone();
                workers.push(tokio::spawn(async move {
                    let res = self_copy.get_dependency_tree(pkg_name, Some(range)).await;

                    // Increase completed
                    self_copy.sender.send(()).await.ok();

                    res
                }));
            }

            loop {
                match workers.next().await {
                    Some(result) => result??,
                    None => break,
                }
            }

            self.sender.send(()).await.ok();

            Ok(())
        }
        .boxed()
    }
}
