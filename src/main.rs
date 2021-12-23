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

pub mod http_manager;
pub mod package;

// Std Imports
use std::fs::File;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, sync::atomic::AtomicI16};

use anyhow::{anyhow, Result};
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use speedy::{Readable, Writable};

use indicatif::{ProgressBar, ProgressStyle};
use package::{Bin, Engine, Package, Version};
use ssri::Integrity;
use tokio::{
    self,
    sync::{mpsc, Mutex},
};

use colored::Colorize;

use std::process::Command;
use std::sync::atomic::Ordering;

#[derive(Clone)]
pub struct Main {
    pub dependencies: Arc<Mutex<Vec<(Package, Version)>>>,
    pub total_dependencies: Arc<AtomicI16>,
    pub sender: mpsc::Sender<()>,
}

#[derive(Debug, Clone, Writable, Readable)]
struct VoltResponse {
    version: String,                    // the latest version of the package
    versions: Vec<String>,              // list of versions of the package
    tree: HashMap<String, VoltPackage>, // the flattened dependency tree for the latest version of the package <name@version, data>
}

#[derive(Debug, Clone, Writable, Readable)]
struct VoltPackage {
    pub name: String,                                       // the name of the package
    pub version: String,                                    // the version of the package
    pub integrity: String, // sha-1 base64 encoded hash or the "integrity" field if it exists
    pub tarball: String,   // url to the tarball to fetch
    pub bin: Option<Bin>,  // binary scripts required by / for the package
    pub scripts: Option<HashMap<String, String>>, // scripts required by / for the package
    pub dependencies: Option<HashMap<String, String>>, // dependencies of the package
    pub peer_dependencies: Option<HashMap<String, String>>, // peer dependencies of the package
    pub peer_dependencies_meta: Option<HashMap<String, String>>, // peer dependencies metadata of the package
    pub optional_dependencies: Option<HashMap<String, String>>, // optional dependencies of the package
    pub overrides: Option<HashMap<String, String>>,             // overrides specific to the package
    pub engines: Option<Engine>,  // engines compatible with the package
    pub os: Option<Vec<String>>,  // operating systems compatible with the package
    pub cpu: Option<Vec<String>>, // cpu architectures compatible with the package
}
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PackageLock {
    name: String,
    version: String,
    #[serde(rename = "lockfileVersion")]
    lockfile_version: u8,
    #[serde(default)]
    packages: HashMap<String, LockFilePackage>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LockFilePackage {
    version: String,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[tokio::main]
async fn main() {
    let contents = r##"{
    "name": "installs",
    "version": "0.1.0",
    "main": "index.js",
    "author": "XtremeDevX <xtremedevx@gmail.com>",
    "license": "Mit"
}"##;

    let mut file = File::create("installs/package.json").unwrap();
    file.write_all(contents.as_bytes()).unwrap();

    let _ = std::fs::remove_file("installs/package-lock.json");

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

    std::env::set_current_dir("installs/").unwrap();

    Command::new("npm")
        .arg("i")
        .arg("--package-lock-only")
        .arg(input_packages[0].clone())
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    let mut file = File::open("package-lock.json").unwrap();

    let mut package_lock_contents: String = String::new();

    file.read_to_string(&mut package_lock_contents).unwrap();

    let lock = serde_json::from_str::<PackageLock>(&package_lock_contents).unwrap();

    let cleaned_up_lockfile = PackageLock {
        name: lock.name,
        version: lock.version,
        lockfile_version: lock.lockfile_version,
        packages: lock
            .packages
            .into_iter()
            .map(|(name, package)| {
                (
                    name.replace("node_modules/", ""),
                    LockFilePackage {
                        version: package.version,
                        dependencies: package
                            .dependencies
                            .into_iter()
                            .map(|(name, version)| (name, version))
                            .collect(),
                    },
                )
            })
            .collect(),
    };

    std::process::exit(0);

    // let progress_bar = ProgressBar::new(1);

    // progress_bar.set_style(
    //     ProgressStyle::default_bar()
    //         .progress_chars("=> ")
    //         .template(&format!(
    //             "{} [{{bar:40.magenta/green}}] {{msg:.blue}}",
    //             "Fetching dependencies".bright_blue()
    //         )),
    // );

    // let mut done: i16 = 0;

    // while rx.recv().await.is_some() {
    //     done += 1;
    //     let total = add.total_dependencies.load(Ordering::Relaxed);
    //     if done == total {
    //         break;
    //     }
    //     progress_bar.set_length(total as u64);
    //     progress_bar.set_position(done as u64);
    // }
    // progress_bar.finish_with_message("[DONE]");

    // println!(
    //     "Resolved {} dependencies.",
    //     add.dependencies
    //         .lock()
    //         .map(|deps| deps
    //             .iter()
    //             .map(|(dep, ver)| format!("{}: {}", dep.name, ver.version))
    //             .count())
    //         .await
    // );

    // let dependencies = Arc::try_unwrap(add.dependencies).unwrap().into_inner();

    // let mut version_data: HashMap<String, VoltPackage> = HashMap::new();

    // let mut parent_version: String = String::new();
    // let mut parent_versions: Vec<String> = vec![];

    // for dependency in dependencies.iter() {
    //     let mut deps: Option<HashMap<String, String>> = Some(dependency.1.dependencies.clone());

    //     if deps.as_ref().unwrap().len() == 0 {
    //         deps = None;
    //     }

    //     let d1 = dependency.1.clone();

    //     let integrity;

    //     if d1.dist.integrity != "" {
    //         let ssri_parsed: Integrity = d1.dist.integrity.clone().parse().unwrap();
    //         integrity = ssri_parsed.to_string();
    //     } else {
    //         let ssri_parsed: Integrity =
    //             format!("sha1-{}", d1.dist.shasum.clone()).parse().unwrap();
    //         integrity = ssri_parsed.to_string();
    //     }

    //     // convert each of these into something that satisfies Option
    //     let peer_dependencies = if d1.peer_dependencies.is_empty() {
    //         None
    //     } else {
    //         Some(d1.peer_dependencies)
    //     };

    //     let optional_dependencies = if d1.optional_dependencies.is_empty() {
    //         None
    //     } else {
    //         Some(d1.optional_dependencies)
    //     };

    //     let scripts = if d1.scripts.is_empty() {
    //         None
    //     } else {
    //         Some(d1.scripts)
    //     };

    //     let bin: Option<Bin>;

    //     match d1.bin {
    //         Bin::String(string) => {
    //             if string.is_empty() {
    //                 bin = None;
    //             } else {
    //                 bin = Some(Bin::String(string));
    //             }
    //         }
    //         Bin::Map(map) => {
    //             if map.is_empty() {
    //                 bin = None;
    //             } else {
    //                 bin = Some(Bin::Map(map));
    //             }
    //         }
    //     }

    //     let mut overrides: Option<HashMap<String, String>> = Some(d1.overrides);

    //     if overrides.as_ref().unwrap().is_empty() {
    //         overrides = None;
    //     }

    //     let engines: Option<Engine>;

    //     match d1.engines {
    //         Engine::String(string) => {
    //             if string.is_empty() {
    //                 engines = None;
    //             } else {
    //                 engines = Some(Engine::String(string));
    //             }
    //         }
    //         Engine::List(vec) => {
    //             if vec.is_empty() {
    //                 engines = None;
    //             } else {
    //                 engines = Some(Engine::List(vec));
    //             }
    //         }
    //         Engine::Map(map) => {
    //             if map.is_empty() {
    //                 engines = None;
    //             } else {
    //                 engines = Some(Engine::Map(map));
    //             }
    //         }
    //     }

    //     let mut os: Option<Vec<String>> = Some(d1.os);

    //     if os.as_ref().unwrap().is_empty() {
    //         os = None;
    //     }

    //     let mut cpu: Option<Vec<String>> = Some(d1.cpu);

    //     if cpu.as_ref().unwrap().is_empty() {
    //         cpu = None;
    //     }

    //     let package = VoltPackage {
    //         name: d1.name.clone(),
    //         version: d1.version.clone(),
    //         dependencies: deps,
    //         peer_dependencies,
    //         integrity,
    //         bin,
    //         scripts,
    //         tarball: d1.dist.tarball.clone(),
    //         peer_dependencies_meta: None,
    //         optional_dependencies,
    //         overrides,
    //         engines,
    //         os,
    //         cpu,
    //     };

    //     if d1.name.clone() == input_packages[0].clone() {
    //         parent_version = d1.version.clone();

    //         parent_versions = dependency
    //             .0
    //             .versions
    //             .keys()
    //             .map(|v| v.to_owned())
    //             .collect::<Vec<String>>();

    //         parent_versions.sort();
    //     }

    //     version_data.insert(
    //         format!("{}@{}", d1.name.clone(), d1.version.clone()),
    //         package,
    //     );
    // }

    // let res: VoltResponse = VoltResponse {
    //     version: parent_version,
    //     versions: parent_versions,
    //     tree: version_data,
    // };

    // let bytes = res.write_to_vec().unwrap();

    // let mut file =
    //     File::create(PathBuf::from("packages").join(format!("{}.sp", input_packages[0].clone())))
    //         .unwrap();

    // file.write_all(&bytes).unwrap();

    // deser
    // drop(file);

    // let mut file = File::open(format!("packages\\{}.rkyv", input_packages[0].clone())).unwrap();

    // let start = std::time::Instant::now();

    // let mut bytes: Vec<u8> = vec![];

    // file.read_to_end(&mut bytes).unwrap();

    // let archived = unsafe { archived_root::<VoltResponse>(&bytes[..]) };
    // let deserialized: VoltResponse = VoltResponse::read_from_buffer(&bytes).unwrap();
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
        let package = http_manager::get_package(package_name).await;

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
            if version_range.is_some() {
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

            while let Some(result) = workers.next().await {
                result??
            }

            self.sender.send(()).await.ok();

            Ok(())
        }
        .boxed()
    }
}
