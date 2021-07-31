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

use std::fs::{remove_file, OpenOptions};
use std::io::{Seek, SeekFrom};
// Std Imports
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicI16};

use anyhow::{anyhow, Result};
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};

use serde::{Deserialize, Serialize};

use indicatif::{ProgressBar, ProgressStyle};
use lz4::EncoderBuilder;
use package::{Package, Version};
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
struct BincodeVoltResponse {
    latest: String,
    schema: u8,
    versions: HashMap<String, HashMap<String, BincodeVoltPackage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BincodeVoltPackage {
    sha1: Vec<u8>,
    sha512: Option<Vec<u8>>,
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
        "Loaded {} dependencies.",
        add.dependencies
            .lock()
            .map(|deps| deps
                .iter()
                .map(|(dep, ver)| format!("{}: {}", dep.name, ver.version))
                .collect::<Vec<_>>()
                .len())
            .await
    );

    // let mut selected_version: VoltPackage = VoltPackage {
    //     sha1: String::new(),
    //     peer_dependencies: None,
    //     dependencies: None,
    //     bin: None,
    //     integrity: String::new(),
    // };

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

    let mut versions: HashMap<String, HashMap<String, BincodeVoltPackage>> = HashMap::new();
    versions.insert(ds_clone.clone().latest, HashMap::new());

    let mut bincode_struct: BincodeVoltResponse = BincodeVoltResponse {
        latest: ds_clone.clone().latest,
        schema: ds_clone.clone().schema,
        versions,
    };

    for (name, package) in res.versions.get(&res.latest).unwrap().iter() {
        let sha512;

        if package.integrity.is_some() {
            sha512 = Some(
                base64::decode(package.integrity.as_ref().unwrap().replace("sha512-", "")).unwrap(),
            );
        } else {
            sha512 = None;
        }

        let bincode_package: BincodeVoltPackage = BincodeVoltPackage {
            sha1: package
                .sha1
                .as_bytes()
                .chunks(2)
                .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
                .collect(),
            sha512,
        };

        bincode_struct
            .versions
            .get_mut(&ds_clone.clone().latest)
            .unwrap()
            .insert(name.to_string(), bincode_package);
    }

    let mut input_file = OpenOptions::new()
        .write(true)
        .create(true)
        .read(true)
        .truncate(true)
        .open(format!(
            r"temp\{}.bin",
            input_packages.clone()[0].to_string()
        ))
        .unwrap();

    input_file.seek(SeekFrom::Start(0)).unwrap();

    let output_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .read(true)
        .open(format!(
            r"bin\{}.bin",
            input_packages.clone()[0].to_string()
        ))
        .unwrap();

    bincode::serialize_into(&input_file, &bincode_struct).unwrap();

    let mut encoder = EncoderBuilder::new().level(4).build(output_file).unwrap();

    std::io::copy(&mut input_file, &mut encoder).unwrap();

    remove_file(format!(
        r"temp\{}.bin",
        input_packages.clone()[0].to_string()
    ))
    .unwrap();

    let (_, _) = encoder.finish();
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
