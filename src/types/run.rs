use std::{collections::HashMap, fmt, result::Result as StdResult, str::FromStr};

use ckb_types::H256;
use rand::{distributions::WeightedIndex, thread_rng};
use rand_distr::{Distribution as _, Normal};
use serde::{Deserialize, Serialize};

use super::{LockInfo, LockScriptId};
use crate::error::{Error, Result};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunEnv {
    pub(crate) delay_blocks: u64,
    pub(crate) generator: GeneratorConfig,
    pub(crate) client: ClientConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratorConfig {
    pub(crate) inputs_limit: usize,
    pub(crate) inputs_size_normal_distribution: NormalDistributionConfig,
    pub(crate) outputs_limit: usize,
    pub(crate) output_capacity: u32,
    pub(crate) output_min_capacity: u32,
    pub(crate) tx_fee: u64,
    pub(crate) locks_weights: HashMap<LockScriptId, usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientConfig {
    pub(crate) idle_interval: u64,
    pub(crate) success_interval: u64,
    pub(crate) failure_interval: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct NormalDistributionConfig {
    pub(crate) mean: u8,
    pub(crate) std_dev: u8,
}

pub(crate) struct InputSizeGenerator(Normal<f32>);

pub(crate) struct LockGenerator {
    items: Vec<(H256, usize)>,
    index: WeightedIndex<usize>,
}

impl FromStr for RunEnv {
    type Err = serde_yaml::Error;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        serde_yaml::from_str(&s)
    }
}

impl fmt::Display for RunEnv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        serde_yaml::to_string(self)
            .map_err(|_| fmt::Error)
            .and_then(|s| write!(f, "{}", s))
    }
}

impl GeneratorConfig {
    pub(crate) fn input_size_generator(&self) -> Result<InputSizeGenerator> {
        InputSizeGenerator::new(
            self.inputs_size_normal_distribution.mean,
            self.inputs_size_normal_distribution.std_dev,
        )
    }

    pub(crate) fn lock_generator(
        &self,
        accounts: &HashMap<H256, LockInfo>,
    ) -> Result<LockGenerator> {
        let items = accounts
            .iter()
            .map(|(hash, info)| {
                let weight = self.locks_weights.get(&info.id).cloned().unwrap_or(0);
                (hash.to_owned(), weight)
            })
            .collect::<Vec<_>>();
        LockGenerator::new(items)
    }
}

impl InputSizeGenerator {
    fn new(mean: u8, std_dev: u8) -> Result<Self> {
        Normal::new(f32::from(mean), f32::from(std_dev))
            .map_err(Error::runtime)
            .map(Self)
    }

    pub(crate) fn generate(&self) -> usize {
        let mut ret;
        loop {
            ret = self.0.sample(&mut thread_rng());
            if ret > 0.0 && ret < 1000.0 {
                break;
            }
        }
        ret.ceil() as usize
    }
}

impl LockGenerator {
    fn new(items: Vec<(H256, usize)>) -> Result<Self> {
        let index = WeightedIndex::new(items.iter().map(|item| item.1)).map_err(Error::runtime)?;
        Ok(Self { items, index })
    }

    pub(crate) fn generate(&self) -> H256 {
        self.items[self.index.sample(&mut thread_rng())]
            .0
            .to_owned()
    }
}
