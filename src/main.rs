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
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, sync::atomic::AtomicI16};

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use reqwest::{Client, ClientBuilder};
use speedy::{Readable, Writable};

use async_recursion::async_recursion;
use package::{Bin, Engine, Package, Version};
use ssri::Integrity;
use tokio::{self, sync::mpsc};

use std::process::Command;

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
    pub optional: bool,    // whether the package is optional or not
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageLock {
    #[serde(default)]
    packages: HashMap<String, LockFilePackage>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LockFilePackage {
    #[serde(skip)]
    version: String,

    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[async_recursion]
async fn fetch_append_package(
    lockfile: PackageLock,
    package: String,
    details: LockFilePackage,
    client: Client,
    tree: Arc<Mutex<HashMap<String, VoltPackage>>>,
) {
    if package != "" {
        // get package metadata
        let data = http_manager::get_package_version(&package, &details.version, &client).await;

        let integrity;

        if data.dist.integrity != "" {
            let ssri_parsed: Integrity = data.dist.integrity.clone().parse().unwrap();
            integrity = ssri_parsed.to_string();
        } else {
            let ssri_parsed: Integrity = format!("sha1-{}", data.dist.shasum.clone())
                .parse()
                .unwrap();
            integrity = ssri_parsed.to_string();
        }

        let peer_dependencies = if data.peer_dependencies.is_empty() {
            None
        } else {
            Some(data.peer_dependencies)
        };

        let optional_dependencies = if data.optional_dependencies.is_empty() {
            None
        } else {
            Some(data.optional_dependencies)
        };

        let scripts = if data.scripts.is_empty() {
            None
        } else {
            Some(data.scripts)
        };

        let bin: Option<Bin>;

        match data.bin {
            Bin::String(string) => {
                if string.is_empty() {
                    bin = None;
                } else {
                    bin = Some(Bin::String(string));
                }
            }
            Bin::Map(map) => {
                if map.is_empty() {
                    bin = None;
                } else {
                    bin = Some(Bin::Map(map));
                }
            }
        }

        let mut overrides: Option<HashMap<String, String>> = Some(data.overrides);

        if overrides.as_ref().unwrap().is_empty() {
            overrides = None;
        }

        let engines: Option<Engine>;

        match data.engines {
            Engine::String(string) => {
                if string.is_empty() {
                    engines = None;
                } else {
                    engines = Some(Engine::String(string));
                }
            }
            Engine::List(vec) => {
                if vec.is_empty() {
                    engines = None;
                } else {
                    engines = Some(Engine::List(vec));
                }
            }
            Engine::Map(map) => {
                if map.is_empty() {
                    engines = None;
                } else {
                    engines = Some(Engine::Map(map));
                }
            }
        }

        let mut os: Option<Vec<String>> = Some(data.os);

        if os.as_ref().unwrap().is_empty() {
            os = None;
        }

        let mut cpu: Option<Vec<String>> = Some(data.cpu);

        if cpu.as_ref().unwrap().is_empty() {
            cpu = None;
        }

        let mut dependencies = HashMap::new();

        for (name, _) in details.dependencies.iter() {
            for (package, metadata) in lockfile.packages.iter() {
                if package == name || package == &format!("{}/{}", data.name, name) {
                    let mut package = package.clone();

                    let split = package
                        .split("/")
                        .map(|v| v.to_string())
                        .collect::<Vec<String>>();

                    // node_modules/send/ms
                    // node_modules/send/node_modules/express/ms
                    if split.len() > 1 && !package.contains("@") {
                        package = split.last().unwrap().clone();
                    }
                    // node_modules/@babel/core
                    // node_modules/@babel/core/node_modules/semver
                    // node_modules/@babel/eslint-parser/node_modules/eslint-scope
                    else if split.len() > 1 && package.contains("@") {
                        let scope = split[split.len() - 2].clone();

                        if scope.contains('@') {
                            package = format!("{}/{}", scope, split.last().unwrap());
                        } else {
                            package = split.last().unwrap().clone();
                        }
                    }

                    dependencies.insert(package.clone(), metadata.version.clone());

                    if !tree.lock().unwrap().contains_key(&format!(
                        "{}@{}",
                        package,
                        metadata.version.clone()
                    )) {
                        fetch_append_package(
                            lockfile.clone(),
                            package,
                            metadata.clone(),
                            client.clone(),
                            tree.clone(),
                        )
                        .await;
                    }
                }
            }
        }

        let dependencies_option: Option<HashMap<String, String>>;

        if dependencies.is_empty() {
            dependencies_option = None;
        } else {
            dependencies_option = Some(dependencies);
        }

        tree.lock().unwrap().insert(
            format!("{}@{}", data.name, data.version),
            VoltPackage {
                name: data.name,
                version: data.version,
                optional: false,
                integrity,
                tarball: data.dist.tarball,
                bin,
                scripts,
                dependencies: dependencies_option,
                peer_dependencies,
                peer_dependencies_meta: None,
                optional_dependencies,
                overrides,
                engines,
                os,
                cpu,
            },
        );
    }
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

    std::env::set_current_dir("installs/").unwrap();

    if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args([
                "/C",
                &format!("pnpm add --lockfile-only {}", input_packages[0].clone()),
            ])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
    } else {
        Command::new("pnpm")
            .arg("add")
            .arg("--lockfile-only")
            .arg(input_packages[0].clone())
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
    }

    let mut file = File::open("pnpm-lock.yaml").unwrap();

    let mut package_lock_contents: String = String::new();

    file.read_to_string(&mut package_lock_contents).unwrap();

    let lock = serde_yaml::from_str::<PackageLock>(&package_lock_contents).unwrap();

    let cleaned_up_lockfile = PackageLock {
        packages: lock
            .packages
            .into_iter()
            .map(|(name, package)| {
                (
                    // express/1.2.3
                    if !name.clone()[1..].starts_with("@") {
                        name[1..].replace("/", "@")
                    } else {
                        let cleaned_name = &name[1..];
                        // @babel/core/7.0.0
                        let split = cleaned_name.split('/').collect::<Vec<&str>>();
                        format!(
                            "{}@{}",
                            split[..split.len() - 1].join("/"),
                            split.last().unwrap()
                        )
                    },
                    LockFilePackage {
                        version: name.split("/").last().unwrap().to_string(),
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

    println!("{:#?}", cleaned_up_lockfile);

    let parent_package_res = http_manager::get_package(&input_packages[0]).await;

    let mut parent_package_version = String::new();

    for (name, data) in cleaned_up_lockfile.packages.iter() {
        if name == &input_packages[0] {
            parent_package_version = data.version.clone();
        }
    }

    let tree: HashMap<String, VoltPackage> = HashMap::new();

    let mut response: VoltResponse = VoltResponse {
        version: parent_package_version,
        versions: parent_package_res.versions.into_keys().collect(),
        tree: HashMap::new(),
    };

    let client = ClientBuilder::new().use_rustls_tls().build().unwrap();

    let mut workers = FuturesUnordered::new();

    let shared_tree = Arc::new(Mutex::new(tree));

    for (package, data) in cleaned_up_lockfile.packages.iter() {
        let package = package.clone();

        let cleaned_up_lockfile_instance = cleaned_up_lockfile.clone();
        let data_instance = data.clone();
        let client_instance = client.clone();
        let tree_instance = shared_tree.clone();

        workers.push(tokio::spawn(async move {
            fetch_append_package(
                cleaned_up_lockfile_instance,
                package.to_string(),
                data_instance,
                client_instance,
                tree_instance,
            )
            .await;
        }));
    }

    while workers.next().await.is_some() {}

    response.tree = shared_tree.lock().unwrap().clone();

    println!("{:#?}", response);

    let bytes = response.write_to_vec().unwrap();

    std::env::set_current_dir("/home/xtremedevx/dev/volt/registry").unwrap();

    let mut file =
        File::create(PathBuf::from("packages").join(format!("{}.sp", input_packages[0].clone())))
            .unwrap();

    file.write_all(&bytes).unwrap();
}
