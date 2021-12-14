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

// Std Imports
use std::collections::HashMap;

// Library Imports
use serde::{Deserialize, Serialize};
use speedy::{Readable, Writable};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    #[serde(rename = "_id")]
    pub id: Option<String>,
    #[serde(rename = "_rev")]
    pub rev: Option<String>,
    pub name: String,
    #[serde(rename = "dist-tags")]
    pub dist_tags: DistTags,
    pub versions: HashMap<String, Version>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DistTags {
    pub latest: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Version {
    pub name: String,
    pub version: String,
    pub description: String,
    pub scripts: HashMap<String, String>,
    pub dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub optional_dependencies: HashMap<String, String>,
    pub overrides: HashMap<String, String>,
    pub engines: HashMap<String, String>,
    pub bin: Bin,
    pub os: Vec<String>,
    pub cpu: Vec<String>,
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_nodeVersion")]
    pub node_version: String,
    #[serde(rename = "_npmVersion")]
    pub npm_version: String,
    pub dist: Dist,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Readable, Writable)]
#[serde(untagged)]
pub enum Bin {
    String(String),
    Map(HashMap<String, String>),
}

impl Default for Bin {
    fn default() -> Self {
        Bin::String(String::new())
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Engines {
    pub node: String,
    pub npm: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Dist {
    pub integrity: String,
    pub shasum: String,
    pub tarball: String,
    pub file_count: i64,
    pub unpacked_size: i64,
    #[serde(rename = "npm-signature")]
    pub npm_signature: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Maintainer {
    pub name: String,
    pub email: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NpmUser {
    pub name: String,
    pub email: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Directories {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NpmOperationalInternal {
    pub host: String,
    pub tmp: String,
}
