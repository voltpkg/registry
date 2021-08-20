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
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicI16};

use anyhow::{anyhow, Result};
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};

use lz4::EncoderBuilder;
use serde::{Deserialize, Serialize};

use indicatif::{ProgressBar, ProgressStyle};
use package::{Package, Version};
use ssri::Integrity;
use tokio::{
    self,
    sync::{mpsc, Mutex},
};

use colored::Colorize;

use std::sync::atomic::Ordering;

use crate::api::{VoltPackage, VoltResponse};

#[derive(Clone)]
pub struct Main {
    pub dependencies: Arc<Mutex<Vec<(Package, Version)>>>,
    pub total_dependencies: Arc<AtomicI16>,
    pub sender: mpsc::Sender<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JSONVoltResponse {
    latest: String,
    schema: u8,
    #[serde(flatten)]
    versions: HashMap<String, HashMap<String, JSONVoltPackage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JSONVoltPackage {
    pub integrity: String,
    pub tarball: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<Vec<String>>,
}

fn compress(source: &Path, destination: &Path) {
    let mut input_file = File::open(source).unwrap();
    let output_file = File::create(destination).unwrap();
    let mut encoder = EncoderBuilder::new().level(3).build(output_file).unwrap();
    std::io::copy(&mut input_file, &mut encoder).unwrap();
    let (_output, result) = encoder.finish();
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
                "{} [{{bar:40.magenta/blue}}] {{msg:.blue}}",
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

    let mut version_spec: String = String::new();

    let dependencies = Arc::try_unwrap(add.dependencies).unwrap().into_inner();

    let mut version_data: HashMap<String, VoltPackage> = HashMap::new();

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

        let mut integrity = None;

        if d1.dist.integrity != String::new() {
            integrity = Some(d1.clone().dist.integrity);
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

        let package = VoltPackage {
            peer_dependencies: pds,
            dependencies: deps,
            integrity,
            bin: None,
            sha1: d1.clone().dist.shasum,
            tarball: d1.clone().dist.tarball,
        };

        if dependency.1.clone().name == input_packages.clone()[0].to_string() {
            // selected_version = package.clone();
            version_spec = d1.clone().version;
        }

        version_data.insert(
            format!("{}@{}", d1.clone().name, d1.clone().version),
            package,
        );
    }

    let mut map = HashMap::new();

    map.insert(version_spec.clone(), version_data);

    let res: VoltResponse = VoltResponse {
        schema: 0,
        latest: version_spec,
        versions: map,
    };

    let ds_clone = res.clone();

    let mut versions: HashMap<String, HashMap<String, JSONVoltPackage>> = HashMap::new();
    versions.insert(ds_clone.clone().latest, HashMap::new());

    let mut json_struct: JSONVoltResponse = JSONVoltResponse {
        latest: ds_clone.clone().latest,
        schema: ds_clone.clone().schema,
        versions,
    };

    let mut name_hash = String::new();

    for (name, package) in res.versions.get(&res.latest).unwrap().iter() {
        let hash_string = if let Some(integrity) = &package.integrity {
            integrity.clone()
        } else {
            format!("sha1-{}", base64::encode(package.clone().sha1))
        };

        let integrity: Integrity = hash_string.parse().unwrap();

        let algo = integrity.pick_algorithm();

        let hash = integrity
            .hashes
            .into_iter()
            .find(|h| h.algorithm == algo)
            .map(|h| Integrity { hashes: vec![h] })
            .map(|i| i.to_hex().1)
            .unwrap();

        let split = name.split("@").collect::<Vec<&str>>();

        if name.starts_with("@") {
            let clean_name = format!("@{}", split[1]);

            if input_packages[0] == clean_name {
                match algo {
                    ssri::Algorithm::Sha1 => {
                        name_hash = format!("sha1-{}", hash);
                    }
                    ssri::Algorithm::Sha512 => {
                        name_hash = format!("sha512-{}", hash);
                    }
                    _ => {}
                }
            }
        } else {
            let clean_name = split[0];

            if input_packages[0] == clean_name {
                match algo {
                    ssri::Algorithm::Sha1 => {
                        name_hash = format!("sha1-{}", hash);
                    }
                    ssri::Algorithm::Sha512 => {
                        name_hash = format!("sha512-{}", hash);
                    }
                    _ => {}
                }
            }
        }

        let json_package: JSONVoltPackage = JSONVoltPackage {
            integrity: hash_string,
            bin: package.bin.clone(),
            tarball: package.tarball.clone(),
            peer_dependencies: package.peer_dependencies.clone(),
            dependencies: package.dependencies.clone(),
        };

        json_struct
            .versions
            .get_mut(&ds_clone.clone().latest)
            .unwrap()
            .insert(name.to_string(), json_package);
    }

    let mut output_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(
            Path::new("temp")
                .join(name_hash.clone())
                .with_extension("json"),
        )
        .unwrap();

    let json_data = serde_json::to_string(&json_struct).unwrap();

    output_file.write(json_data.as_bytes()).unwrap();

    compress(
        &Path::new("temp")
            .join(name_hash.clone())
            .with_extension("json"),
        &Path::new("packages")
            .join(name_hash.clone())
            .with_extension("json"),
    );
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
