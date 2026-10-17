//! vl 打洞策略层：移植 KAMUCL-1.1.21 `src/main/core/voxlink/punchPolicy.ts`(152 行) 与
//! `src/main/core/voxlink/punchRounds.ts`(68 行)；上游为 VoxLink
//! network/{PunchParams,PunchTuner,PortPredictor,NatClass,PunchStrategySelector,
//! PunchFailureClassifier}.java 与 ConnectionManager.java，revision 924845e。
//! 模板类型 PunchProfile 及 8 个模板常量来自兄弟模块 `crate::vl::punchprofiles`
//! （punchProfiles.ts，本文件不复制其数值）。
//!
//! 对照表（本文件项 → 原版行号）：
//! NatClass/symmetric → punchPolicy.ts:6-7；recommendProfile → :8-16；
//! PunchStrategy/selectStrategy → :17-26；TUNER → :28-38；
//! PunchParams/fromProfile → :39-58；FailureReason/PunchResult/classifyFailure → :59-74；
//! nextParams → :75-109；PREDICTOR → :111-118；linearRegressionPredict → :119-128；
//! deltaPredict → :129-137；predict/PredictResult → :138-144；generateTargetPorts → :145-152；
//! ZERO_RECV_FINAL_ROUND_LIMIT → punchRounds.ts:5；PREDICTION_OFF_CAP → :6；
//! BACKOFF_DELAYS_MS → :7；PROFILE_SWITCH_COOLDOWN_MS → :8；
//! PunchRounds::{maxCycles,classify,switchProfile,record,terminal,advance} → :9-67。
//!
//! 数值统一用 f64（JS number 即 f64），Math.round/trunc/floor 逐一对应，不做“修正”。
//! 出站 JSON 用 js_number_value 保持 JSON.stringify 的整数字面量（100 而非 100.0）。

#![allow(non_camel_case_types)]

use serde::{Serialize, Serializer};
use serde_json::{Map, Number, Value};
use std::sync::Arc;

use crate::vl::punchprofiles::{
    PunchProfile, AGGRESSIVE, DEFAULT, EASY_SYM_DUAL, FAST_LANE, HARDSYM, V100,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum NatClass {
    UNKNOWN,
    CONE,
    EASY_SYM,
    HARD_SYM,
}

pub fn symmetric(nat: NatClass) -> bool {
    nat == NatClass::EASY_SYM || nat == NatClass::HARD_SYM
}

pub fn recommend_profile(local: NatClass, remote: NatClass) -> PunchProfile {
    if local == NatClass::UNKNOWN || remote == NatClass::UNKNOWN {
        return AGGRESSIVE.clone();
    }
    if symmetric(local) && symmetric(remote) {
        return if local == NatClass::EASY_SYM && remote == NatClass::EASY_SYM {
            EASY_SYM_DUAL.clone()
        } else {
            HARDSYM.clone()
        };
    }
    if local == NatClass::CONE && remote == NatClass::CONE {
        return FAST_LANE.clone();
    }
    if local == NatClass::EASY_SYM || remote == NatClass::EASY_SYM {
        return V100.clone();
    }
    if local == NatClass::HARD_SYM || remote == NatClass::HARD_SYM {
        return AGGRESSIVE.clone();
    }
    DEFAULT.clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum PunchStrategy {
    DIRECT_ONLY,
    DIRECT_WITH_REVERSE_PARALLEL,
    REVERSE_FIRST,
    REVERSE_ONLY,
    REVERSE_THEN_FORWARD,
    RELAY_FALLBACK_FAST,
    PARALLEL_FROM_START,
}

pub fn select_strategy(
    local: Option<NatClass>,
    remote: Option<NatClass>,
    cycle: f64,
    legacy: bool,
) -> PunchStrategy {
    if legacy {
        return PunchStrategy::DIRECT_ONLY;
    }
    let (local, remote) = match (local, remote) {
        (Some(local), Some(remote)) => (local, remote),
        _ => return PunchStrategy::DIRECT_WITH_REVERSE_PARALLEL,
    };
    if local == NatClass::UNKNOWN || remote == NatClass::UNKNOWN {
        let other = if local == NatClass::UNKNOWN {
            remote
        } else {
            local
        };
        return if symmetric(other) {
            PunchStrategy::REVERSE_FIRST
        } else {
            PunchStrategy::DIRECT_WITH_REVERSE_PARALLEL
        };
    }
    if (local == NatClass::CONE && remote == NatClass::HARD_SYM)
        || (local == NatClass::HARD_SYM && remote == NatClass::CONE)
    {
        return if cycle == 0.0 {
            PunchStrategy::REVERSE_ONLY
        } else {
            PunchStrategy::REVERSE_THEN_FORWARD
        };
    }
    if local == remote {
        return if local == NatClass::CONE {
            PunchStrategy::DIRECT_ONLY
        } else {
            PunchStrategy::DIRECT_WITH_REVERSE_PARALLEL
        };
    }
    PunchStrategy::REVERSE_FIRST
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Tuner {
    #[serde(serialize_with = "ser_f64")]
    pub max_port_range: f64,
    #[serde(serialize_with = "ser_f64")]
    pub max_timeout_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub min_send_interval_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub late_cycle_timeout_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub ack_retries_on_timeout: f64,
    #[serde(serialize_with = "ser_f64")]
    pub prediction_delta_threshold: f64,
    #[serde(serialize_with = "ser_f64")]
    pub port_range_multiplier: f64,
    #[serde(serialize_with = "ser_f64")]
    pub timeout_increment_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub send_interval_divisor: f64,
}

pub const TUNER: Tuner = Tuner {
    max_port_range: 500.0,
    max_timeout_ms: 30000.0,
    min_send_interval_ms: 50.0,
    late_cycle_timeout_ms: 5000.0,
    ack_retries_on_timeout: 3.0,
    prediction_delta_threshold: 100.0,
    port_range_multiplier: 2.0,
    timeout_increment_ms: 4000.0,
    send_interval_divisor: 2.0,
};

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PunchParams {
    #[serde(serialize_with = "ser_f64")]
    pub port_range: f64,
    #[serde(serialize_with = "ser_f64")]
    pub timeout_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub send_interval: f64,
    #[serde(serialize_with = "ser_f64")]
    pub ack_retries: f64,
    pub skip_direct_punch: bool,
    pub reuse_successful_sockets: bool,
    #[serde(serialize_with = "ser_f64_list_opt")]
    pub successful_port_range: Option<Vec<f64>>,
    #[serde(serialize_with = "ser_f64")]
    pub send_min_rounds: f64,
    #[serde(serialize_with = "ser_f64")]
    pub send_min_pass: f64,
    pub hard_sym_spray: bool,
    pub easy_sym_bomb: bool,
    #[serde(serialize_with = "ser_f64")]
    pub spray_port_count_min: f64,
    #[serde(serialize_with = "ser_f64")]
    pub spray_port_count_max: f64,
    #[serde(serialize_with = "ser_f64")]
    pub spray_packets_per_port: f64,
    #[serde(serialize_with = "ser_f64")]
    pub spray_port_interval_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub spray_decay_numerator: f64,
    #[serde(serialize_with = "ser_f64")]
    pub spray_decay_floor: f64,
    #[serde(serialize_with = "ser_f64")]
    pub bomb_window: f64,
    #[serde(serialize_with = "ser_f64")]
    pub bomb_round_interval_ms: f64,
    #[serde(serialize_with = "ser_f64")]
    pub bomb_duration_ms: f64,
}

pub fn from_profile(p: &PunchProfile) -> PunchParams {
    PunchParams {
        port_range: p.port_prediction_max_range as f64,
        timeout_ms: p.punch_timeout_ms as f64,
        send_interval: p.send.interval_ms as f64,
        ack_retries: 1.0,
        skip_direct_punch: false,
        reuse_successful_sockets: false,
        successful_port_range: None,
        send_min_rounds: 0.0,
        send_min_pass: 0.0,
        hard_sym_spray: profile_name(p) == "HARDSYM",
        easy_sym_bomb: profile_name(p) == "EASY_SYM_DUAL",
        spray_port_count_min: p.sym.hard_sym_spray_port_min as f64,
        spray_port_count_max: p.sym.hard_sym_spray_port_max as f64,
        spray_packets_per_port: p.sym.hard_sym_packets_per_port as f64,
        spray_port_interval_ms: p.sym.hard_sym_port_interval_ms as f64,
        spray_decay_numerator: p.sym.hard_sym_decay_numerator as f64,
        spray_decay_floor: p.sym.hard_sym_decay_floor as f64,
        bomb_window: p.sym.easy_sym_bomb_window as f64,
        bomb_round_interval_ms: p.sym.easy_sym_round_interval_ms as f64,
        bomb_duration_ms: p.sym.easy_sym_bomb_duration_ms as f64,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum FailureReason {
    NO_RESPONSE,
    PREDICTION_OFF,
    RESPONSE_NO_ACK,
    FIREWALL_DETECTED,
    PARTIAL_SUCCESS,
    ACK_TIMEOUT,
}

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PunchResult {
    #[serde(serialize_with = "ser_f64")]
    pub sockets_tried: f64,
    #[serde(serialize_with = "ser_f64")]
    pub sockets_received_punch: f64,
    #[serde(serialize_with = "ser_f64")]
    pub sockets_received_ack: f64,
    #[serde(serialize_with = "ser_f64")]
    pub prediction_delta: f64,
    #[serde(serialize_with = "ser_f64")]
    pub elapsed_ms: f64,
    pub firewall_detected: bool,
    pub port_prediction_active: bool,
    pub success: bool,
}

pub fn classify_failure(r: Option<&PunchResult>) -> FailureReason {
    // 判定顺序不可调 punchPolicy.ts:67
    let r = match r {
        Some(r) => r,
        None => return FailureReason::NO_RESPONSE,
    };
    if r.port_prediction_active
        && r.sockets_received_punch == 0.0
        && r.sockets_received_ack == 0.0
        && !r.firewall_detected
    {
        return FailureReason::PREDICTION_OFF;
    }
    if r.prediction_delta > TUNER.prediction_delta_threshold {
        return FailureReason::PREDICTION_OFF;
    }
    if !js_truthy(r.sockets_received_punch) && !js_truthy(r.sockets_received_ack) {
        return if r.firewall_detected {
            FailureReason::FIREWALL_DETECTED
        } else {
            FailureReason::NO_RESPONSE
        };
    }
    if r.sockets_received_punch > 0.0 && r.sockets_received_ack == 0.0 {
        return FailureReason::RESPONSE_NO_ACK;
    }
    if r.sockets_received_ack > 0.0 && !r.success {
        return FailureReason::ACK_TIMEOUT;
    }
    if r.sockets_received_punch > 0.0 && r.sockets_received_punch < r.sockets_tried {
        return FailureReason::PARTIAL_SUCCESS;
    }
    FailureReason::NO_RESPONSE
}

pub fn next_params(
    profile: &PunchProfile,
    cycle: f64,
    max_cycles: f64,
    failure: Option<FailureReason>,
    result: Option<&PunchResult>,
) -> PunchParams {
    let mut p = from_profile(profile);
    if let Some(failure) = failure {
        match failure {
            FailureReason::NO_RESPONSE => {
                p.port_range = js_min(
                    p.port_range * TUNER.port_range_multiplier,
                    TUNER.max_port_range,
                );
                p.timeout_ms = js_min(
                    p.timeout_ms + TUNER.timeout_increment_ms,
                    TUNER.max_timeout_ms,
                );
                p.send_min_rounds = 3.0;
                p.send_min_pass = 3.0;
            }
            FailureReason::PREDICTION_OFF => {
                p = from_profile(&HARDSYM);
                p.port_range = js_min(
                    p.port_range * TUNER.port_range_multiplier,
                    TUNER.max_port_range,
                );
                p.send_min_rounds = 3.0;
                p.send_min_pass = 3.0;
            }
            FailureReason::RESPONSE_NO_ACK => {
                // 夹住下限 punchPolicy.ts:90
                p.send_interval = js_max(
                    (p.send_interval / TUNER.send_interval_divisor).trunc(),
                    TUNER.min_send_interval_ms,
                );
                p.send_min_pass = 3.0;
            }
            FailureReason::FIREWALL_DETECTED => p.skip_direct_punch = true,
            FailureReason::PARTIAL_SUCCESS => {
                if let Some(result) = result {
                    if result.prediction_delta > 0.0 {
                        p.reuse_successful_sockets = true;
                        p.successful_port_range = Some(vec![
                            result.prediction_delta - 5.0,
                            result.prediction_delta + 5.0,
                        ]);
                    }
                }
            }
            FailureReason::ACK_TIMEOUT => {
                p.ack_retries = TUNER.ack_retries_on_timeout;
                p.send_min_pass = 3.0;
            }
        }
        if let Some(result) = result {
            if result.elapsed_ms > 0.0 {
                if result.elapsed_ms > 10000.0 {
                    p.timeout_ms = js_min((p.timeout_ms * 3.0 / 2.0).trunc(), TUNER.max_timeout_ms);
                } else if result.elapsed_ms < 3000.0 && failure == FailureReason::NO_RESPONSE {
                    p.port_range = js_min(
                        p.port_range * TUNER.port_range_multiplier,
                        TUNER.max_port_range,
                    );
                }
            }
        }
    }
    // 末期收紧超时 punchPolicy.ts:107
    if cycle >= max_cycles - 2.0 && p.timeout_ms > TUNER.late_cycle_timeout_ms {
        p.timeout_ms = js_max(
            (p.timeout_ms * 3.0 / 4.0).trunc(),
            TUNER.late_cycle_timeout_ms,
        );
    }
    p
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct Predictor {
    #[serde(serialize_with = "ser_f64")]
    pub samples_high: f64,
    #[serde(serialize_with = "ser_f64")]
    pub range_high: f64,
    #[serde(serialize_with = "ser_f64")]
    pub samples_mid: f64,
    #[serde(serialize_with = "ser_f64")]
    pub range_mid: f64,
    #[serde(serialize_with = "ser_f64")]
    pub samples_low: f64,
    #[serde(serialize_with = "ser_f64")]
    pub range_low: f64,
    #[serde(serialize_with = "ser_f64")]
    pub range_default: f64,
    #[serde(serialize_with = "ser_f64")]
    pub epsilon: f64,
    #[serde(serialize_with = "ser_f64")]
    pub alpha: f64,
    #[serde(serialize_with = "ser_f64")]
    pub lr_weight: f64,
    #[serde(serialize_with = "ser_f64")]
    pub delta_weight: f64,
    #[serde(serialize_with = "ser_f64")]
    pub min_port: f64,
    #[serde(serialize_with = "ser_f64")]
    pub max_port: f64,
}

pub const PREDICTOR: Predictor = Predictor {
    samples_high: 10.0,
    range_high: 32.0,
    samples_mid: 5.0,
    range_mid: 64.0,
    samples_low: 3.0,
    range_low: 100.0,
    range_default: 200.0,
    epsilon: 1e-9,
    alpha: 0.4,
    lr_weight: 0.6,
    delta_weight: 0.4,
    min_port: 1024.0,
    max_port: 65535.0,
};

pub fn linear_regression_predict(ports: &[f64]) -> f64 {
    if ports.is_empty() {
        return -1.0;
    }
    if ports.len() == 1 {
        return ports[0];
    }
    let (mut x, mut y, mut xy, mut xx) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (index, port) in ports.iter().enumerate() {
        let i = index as f64;
        x += i;
        y += *port;
        xy += i * *port;
        xx += i * i;
    }
    let n = ports.len() as f64;
    let denom = n * xx - x * x;
    if denom.abs() < PREDICTOR.epsilon {
        return js_round(y / n);
    }
    let slope = (n * xy - x * y) / denom;
    js_round(slope * n + (y - slope * x) / n)
}

pub fn delta_predict(ports: &[f64]) -> f64 {
    if ports.len() < 2 {
        return -1.0;
    }
    let mut sorted: Vec<f64> = ports[1..]
        .iter()
        .enumerate()
        .map(|(i, p)| *p - ports[i])
        .collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let trim = (sorted.len() as f64 / 4.0).trunc() as usize;
    let end = sorted.len().saturating_sub(trim);
    let values: &[f64] = if trim < end { &sorted[trim..end] } else { &[] };
    let mut ema = match values.first() {
        Some(value) => *value,
        None => f64::NAN,
    };
    for value in values.iter().skip(1) {
        ema += PREDICTOR.alpha * (*value - ema);
    }
    ports[ports.len() - 1] + js_round(ema)
}

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictResult {
    #[serde(serialize_with = "ser_f64")]
    pub predicted_port: f64,
    #[serde(serialize_with = "ser_f64")]
    pub range: f64,
    pub strategy: String,
}

pub fn predict(ports: &[f64]) -> PredictResult {
    if ports.len() < 2 {
        return PredictResult {
            predicted_port: match ports.first() {
                Some(value) => *value,
                None => -1.0,
            },
            range: PREDICTOR.range_default,
            strategy: if ports.is_empty() {
                "no_samples".to_string()
            } else {
                "single_sample".to_string()
            },
        };
    }
    let lr = linear_regression_predict(ports);
    let delta = delta_predict(ports);
    let value = if lr > 0.0 && delta > 0.0 {
        js_round(lr * PREDICTOR.lr_weight + delta * PREDICTOR.delta_weight)
    } else if lr > 0.0 {
        lr
    } else {
        delta
    };
    let length = ports.len() as f64;
    let range = if length >= PREDICTOR.samples_high {
        PREDICTOR.range_high
    } else if length >= PREDICTOR.samples_mid {
        PREDICTOR.range_mid
    } else if length >= PREDICTOR.samples_low {
        PREDICTOR.range_low
    } else {
        PREDICTOR.range_default
    };
    PredictResult {
        predicted_port: js_max(PREDICTOR.min_port, js_min(PREDICTOR.max_port, value)),
        range,
        strategy: "combined".to_string(),
    }
}

pub fn generate_target_ports(port: f64, range: f64) -> Vec<f64> {
    let lo = js_max(PREDICTOR.min_port, port - range);
    let hi = js_min(PREDICTOR.max_port, port + range);
    let mut out = vec![port];
    let mut offset = 1.0f64;
    while offset <= range {
        if port + offset <= hi {
            out.push(port + offset);
        }
        if port - offset >= lo {
            out.push(port - offset);
        }
        offset += 1.0;
    }
    out
}

pub const ZERO_RECV_FINAL_ROUND_LIMIT: f64 = 20.0;
pub const PREDICTION_OFF_CAP: f64 = 50.0;
pub const BACKOFF_DELAYS_MS: [f64; 3] = [1000.0, 2000.0, 4000.0];
pub const PROFILE_SWITCH_COOLDOWN_MS: f64 = 20000.0;

type LogFn = Arc<dyn Fn(&str) + Send + Sync + 'static>;

#[derive(Clone)]
pub struct PunchRounds {
    log: LogFn,
    pub cycle: f64,
    pub round: f64,
    pub attempt: f64,
    pub received_ever: bool,
    pub prediction_off_count: f64,
    pub profile: PunchProfile,
    pub params: Option<PunchParams>,
    pub local: NatClass,
    pub remote: NatClass,
    pub reachable: f64,
    switched_at: f64,
    last_failure: Option<FailureReason>,
    consecutive: f64,
    latest_failure: Option<FailureReason>,
}

impl PunchRounds {
    pub fn new<F>(log: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        PunchRounds {
            log: Arc::new(log),
            cycle: 0.0,
            round: 0.0,
            attempt: 1.0,
            received_ever: false,
            prediction_off_count: 0.0,
            profile: DEFAULT.clone(),
            params: None,
            local: NatClass::UNKNOWN,
            remote: NatClass::UNKNOWN,
            reachable: 0.0,
            switched_at: 0.0,
            last_failure: None,
            consecutive: 0.0,
            latest_failure: None,
        }
    }

    pub fn max_cycles(&self) -> f64 {
        if !js_truthy(self.reachable) {
            return self.profile.fallback_cycles as f64;
        }
        if symmetric(self.local) {
            js_max(
                2.0,
                js_min(self.reachable, self.profile.max_sym_cycles as f64),
            )
        } else {
            js_max(1.0, js_min(self.reachable, self.profile.max_cycles as f64))
        }
    }

    pub fn classify(&mut self, local: NatClass, remote: NatClass, reachable: f64) {
        if self.local != local || self.remote != remote {
            self.params = None;
        }
        self.local = local;
        self.remote = remote;
        self.reachable = reachable;
        if !js_truthy(self.round) {
            let next = recommend_profile(local, remote);
            self.switch_profile(next);
        }
    }

    fn switch_profile(&mut self, next: PunchProfile) {
        if profile_name(&next) == profile_name(&self.profile) {
            return;
        }
        let now = now_ms();
        if js_truthy(self.switched_at) && now - self.switched_at < PROFILE_SWITCH_COOLDOWN_MS {
            let message = format!(
                "模板切换被 20 秒节流：目标 {}，保留 {}，剩余 {} ms",
                profile_name(&next),
                profile_name(&self.profile),
                js_number_text(PROFILE_SWITCH_COOLDOWN_MS - (now - self.switched_at))
            );
            (self.log)(&message);
            return;
        }
        let differences = profile_differences(&next);
        let message = format!(
            "模板切换：{} → {}；相对 DEFAULT 的全部差异：{}",
            profile_name(&self.profile),
            profile_name(&next),
            differences
        );
        (self.log)(&message);
        self.profile = next;
        self.switched_at = now;
    }

    pub fn record(&mut self, result: &PunchResult) -> FailureReason {
        if result.sockets_received_punch > 0.0 || result.sockets_received_ack > 0.0 {
            self.received_ever = true;
        }
        let reason = classify_failure(Some(result));
        if reason == FailureReason::PREDICTION_OFF {
            self.prediction_off_count += 1.0;
        }
        let cycle = self.cycle;
        let max_cycles = self.max_cycles();
        let profile = self.profile.clone();
        self.params = Some(next_params(
            &profile,
            cycle + 1.0,
            max_cycles,
            Some(reason),
            Some(result),
        ));
        self.latest_failure = Some(reason);
        reason
    }

    pub fn terminal(&self) -> bool {
        !self.received_ever
            && (self.round >= ZERO_RECV_FINAL_ROUND_LIMIT
                || self.prediction_off_count >= PREDICTION_OFF_CAP)
    }

    pub fn advance(&mut self, firewall: bool) -> f64 {
        if !firewall && self.attempt < self.profile.punch_max_attempts as f64 {
            // 指数封顶 4 punchRounds.ts:47
            let exponent = self.attempt - 1.0;
            let exponent = if exponent.is_nan() {
                f64::NAN
            } else if exponent > 4.0 {
                4.0
            } else {
                exponent
            };
            let delay = self.profile.punch_retry_delay_ms as f64 * 2.0f64.powf(exponent);
            self.attempt += 1.0;
            return delay;
        }
        self.attempt = 1.0;
        let old_cycle = self.cycle;
        self.cycle += 1.0;
        if self.cycle >= self.max_cycles() {
            self.cycle = 0.0;
            self.round += 1.0;
            if profile_name(&self.profile) != "HARDSYM" {
                let next = if profile_name(&self.profile) == "AGGRESSIVE" {
                    HARDSYM.clone()
                } else {
                    AGGRESSIVE.clone()
                };
                self.switch_profile(next);
            }
            return 0.0;
        }
        if old_cycle == 0.0
            && profile_name(&self.profile) == "DEFAULT"
            && (symmetric(self.local) || symmetric(self.remote))
        {
            self.switch_profile(AGGRESSIVE.clone());
        }
        if let Some(latest) = self.latest_failure {
            if Some(latest) == self.last_failure {
                self.consecutive += 1.0;
            } else {
                self.last_failure = Some(latest);
                self.consecutive = 1.0;
            }
            if self.consecutive >= 3.0 {
                if latest == FailureReason::NO_RESPONSE {
                    self.switch_profile(HARDSYM.clone());
                } else if latest != FailureReason::FIREWALL_DETECTED {
                    self.switch_profile(AGGRESSIVE.clone());
                }
                self.consecutive = 0.0;
            }
        }
        let capped = js_min(old_cycle, BACKOFF_DELAYS_MS.len() as f64 - 1.0);
        if capped.fract() == 0.0 && capped >= 0.0 && capped < BACKOFF_DELAYS_MS.len() as f64 {
            BACKOFF_DELAYS_MS[capped as usize]
        } else {
            f64::NAN
        }
    }
}

impl Default for PunchRounds {
    fn default() -> Self {
        Self::new(|_message| {})
    }
}

// 保留插入序 punchRounds.ts:29
fn profile_differences(next: &PunchProfile) -> String {
    let next_value = normalize_js(&to_value(next));
    let base_value = normalize_js(&to_value(&DEFAULT));
    let mut differences = Map::new();
    if let (Some(entries), Some(base)) = (next_value.as_object(), base_value.as_object()) {
        for (key, value) in entries {
            let base_text = match base.get(key) {
                Some(found) => found.to_string(),
                None => "undefined".to_string(),
            };
            if value.to_string() != base_text {
                differences.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(differences).to_string()
}

fn profile_name(p: &PunchProfile) -> &str {
    &*p.name
}

fn to_value<T: Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn normalize_js(value: &Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(real) => js_number_value(real),
            None => value.clone(),
        },
        Value::Array(items) => Value::Array(items.iter().map(normalize_js).collect()),
        Value::Object(entries) => {
            let mut out = Map::new();
            for (key, item) in entries {
                out.insert(key.clone(), normalize_js(item));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn js_number_value(real: f64) -> Value {
    if !real.is_finite() {
        return Value::Null;
    }
    if real.fract() == 0.0 && real.abs() < 9_007_199_254_740_992.0 {
        return Value::from(real as i64);
    }
    Number::from_f64(real)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn ser_f64<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    js_number_value(*value).serialize(serializer)
}

fn ser_f64_list_opt<S: Serializer>(
    value: &Option<Vec<f64>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        None => serializer.serialize_none(),
        Some(items) => Value::Array(items.iter().map(|item| js_number_value(*item)).collect())
            .serialize(serializer),
    }
}

fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b {
        a
    } else {
        b
    }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b {
        a
    } else {
        b
    }
}

fn js_truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

fn js_number_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if value == 0.0 {
        "0".to_string()
    } else if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(profile: &PunchProfile) -> &str {
        profile_name(profile)
    }

    fn result(
        sockets_tried: f64,
        punch: f64,
        ack: f64,
        delta: f64,
        elapsed: f64,
        firewall: bool,
        prediction: bool,
        success: bool,
    ) -> PunchResult {
        PunchResult {
            sockets_tried,
            sockets_received_punch: punch,
            sockets_received_ack: ack,
            prediction_delta: delta,
            elapsed_ms: elapsed,
            firewall_detected: firewall,
            port_prediction_active: prediction,
            success,
        }
    }

    // 区分：16 格 NAT 组合的判定先后（UNKNOWN 短路 / 双对称 / CONE×CONE / 单侧对称回落）
    #[test]
    fn recommend_profile_matrix() {
        let rows = [
            NatClass::UNKNOWN,
            NatClass::CONE,
            NatClass::EASY_SYM,
            NatClass::HARD_SYM,
        ];
        let expect = [
            ["AGGRESSIVE", "AGGRESSIVE", "AGGRESSIVE", "AGGRESSIVE"],
            ["AGGRESSIVE", "FAST_LANE", "V100", "AGGRESSIVE"],
            ["AGGRESSIVE", "V100", "EASY_SYM_DUAL", "HARDSYM"],
            ["AGGRESSIVE", "AGGRESSIVE", "HARDSYM", "HARDSYM"],
        ];
        for (index, local) in rows.iter().enumerate() {
            for (column, remote) in rows.iter().enumerate() {
                assert_eq!(
                    name(&recommend_profile(*local, *remote)),
                    expect[index][column],
                    "local={:?} remote={:?}",
                    local,
                    remote
                );
            }
        }
    }

    // 区分：selectStrategy 全 32 格（16 NAT 组合 × cycle 0/1）——UNKNOWN 格取「对端」判对称、
    // CONE×HARD_SYM 的 cycle===0 严格比较、同类型只有 CONE 走 DIRECT_ONLY
    #[test]
    fn select_strategy_matrix() {
        use NatClass::*;
        use PunchStrategy::*;
        let rows = [UNKNOWN, CONE, EASY_SYM, HARD_SYM];
        let expect = [
            [
                [DIRECT_WITH_REVERSE_PARALLEL, DIRECT_WITH_REVERSE_PARALLEL],
                [DIRECT_WITH_REVERSE_PARALLEL, DIRECT_WITH_REVERSE_PARALLEL],
                [REVERSE_FIRST, REVERSE_FIRST],
                [REVERSE_FIRST, REVERSE_FIRST],
            ],
            [
                [DIRECT_WITH_REVERSE_PARALLEL, DIRECT_WITH_REVERSE_PARALLEL],
                [DIRECT_ONLY, DIRECT_ONLY],
                [REVERSE_FIRST, REVERSE_FIRST],
                [REVERSE_ONLY, REVERSE_THEN_FORWARD],
            ],
            [
                [REVERSE_FIRST, REVERSE_FIRST],
                [REVERSE_FIRST, REVERSE_FIRST],
                [DIRECT_WITH_REVERSE_PARALLEL, DIRECT_WITH_REVERSE_PARALLEL],
                [REVERSE_FIRST, REVERSE_FIRST],
            ],
            [
                [REVERSE_FIRST, REVERSE_FIRST],
                [REVERSE_ONLY, REVERSE_THEN_FORWARD],
                [REVERSE_FIRST, REVERSE_FIRST],
                [DIRECT_WITH_REVERSE_PARALLEL, DIRECT_WITH_REVERSE_PARALLEL],
            ],
        ];
        for (index, local) in rows.iter().enumerate() {
            for (column, remote) in rows.iter().enumerate() {
                for (step, cycle) in [0.0f64, 1.0].iter().enumerate() {
                    assert_eq!(
                        select_strategy(Some(*local), Some(*remote), *cycle, false),
                        expect[index][column][step],
                        "local={:?} remote={:?} cycle={}",
                        local,
                        remote,
                        cycle
                    );
                }
            }
        }
        // 区分：legacy 短路优先级高于空值与已知识别
        assert_eq!(select_strategy(None, None, 0.0, true), DIRECT_ONLY);
        assert_eq!(
            select_strategy(Some(CONE), Some(CONE), 0.0, true),
            DIRECT_ONLY
        );
        assert_eq!(
            select_strategy(None, Some(CONE), 0.0, false),
            DIRECT_WITH_REVERSE_PARALLEL
        );
        assert_eq!(
            select_strategy(Some(CONE), None, 0.0, false),
            DIRECT_WITH_REVERSE_PARALLEL
        );
        assert_eq!(
            select_strategy(Some(HARD_SYM), Some(CONE), -1.0, false),
            REVERSE_THEN_FORWARD
        );
    }

    // 区分：PREDICTION_OFF 前置三条件与 firewall 抑制、delta 阈值 100 是 > 不是 >=、
    // 「双零」早于「有回包无 ACK」、末尾兜底 NO_RESPONSE
    #[test]
    fn classify_failure_branches() {
        assert_eq!(classify_failure(None), FailureReason::NO_RESPONSE);
        assert_eq!(
            classify_failure(Some(&result(1.0, 0.0, 0.0, 0.0, 0.0, false, true, false))),
            FailureReason::PREDICTION_OFF
        );
        assert_eq!(
            classify_failure(Some(&result(1.0, 0.0, 0.0, 0.0, 0.0, true, true, false))),
            FailureReason::FIREWALL_DETECTED
        );
        assert_eq!(
            classify_failure(Some(&result(
                1.0, 1.0, 0.0, 101.0, 0.0, false, false, false
            ))),
            FailureReason::PREDICTION_OFF
        );
        assert_eq!(
            classify_failure(Some(&result(
                1.0, 1.0, 0.0, 100.0, 0.0, false, false, false
            ))),
            FailureReason::RESPONSE_NO_ACK
        );
        assert_eq!(
            classify_failure(Some(&result(2.0, 0.0, 0.0, 0.0, 0.0, false, false, false))),
            FailureReason::NO_RESPONSE
        );
        assert_eq!(
            classify_failure(Some(&result(2.0, 0.0, 1.0, 0.0, 0.0, false, false, false))),
            FailureReason::ACK_TIMEOUT
        );
        assert_eq!(
            classify_failure(Some(&result(2.0, 1.0, 2.0, 0.0, 0.0, false, false, false))),
            FailureReason::ACK_TIMEOUT
        );
        assert_eq!(
            classify_failure(Some(&result(2.0, 1.0, 1.0, 0.0, 0.0, false, false, true))),
            FailureReason::PARTIAL_SUCCESS
        );
        assert_eq!(
            classify_failure(Some(&result(1.0, 1.0, 1.0, 0.0, 0.0, false, false, true))),
            FailureReason::NO_RESPONSE
        );
    }

    // 区分：五档升级阶梯各自的字段、200→500 上限、30000 上限、trunc 后再夹 50 下限、
    // elapsed 三支的先后与「仅 NO_RESPONSE 二次放宽」、末期收紧的作用域（无 failure 也生效）
    #[test]
    fn next_params_ladder() {
        let base = from_profile(&DEFAULT);
        assert_eq!(base.port_range, 100.0);
        assert_eq!(base.timeout_ms, 12000.0);
        assert_eq!(base.send_interval, 200.0);

        let p = next_params(&DEFAULT, 0.0, 8.0, Some(FailureReason::NO_RESPONSE), None);
        assert_eq!(p.port_range, 200.0);
        assert_eq!(p.timeout_ms, 16000.0);
        assert_eq!(p.send_min_rounds, 3.0);
        assert_eq!(p.send_min_pass, 3.0);
        assert_eq!(p.send_interval, 200.0);
        assert_eq!(p.ack_retries, 1.0);

        // cycle=6 >= 8-2 收紧为 trunc(16000*3/4)=12000；cycle=5 不收紧
        let p = next_params(&DEFAULT, 6.0, 8.0, Some(FailureReason::NO_RESPONSE), None);
        assert_eq!(p.timeout_ms, 12000.0);
        assert_eq!(p.port_range, 200.0);
        let p = next_params(&DEFAULT, 5.0, 8.0, Some(FailureReason::NO_RESPONSE), None);
        assert_eq!(p.timeout_ms, 16000.0);

        // 无 failure 时仍走末期收紧：trunc(12000*3/4)=9000，portRange 保持 100
        let p = next_params(&DEFAULT, 6.0, 8.0, None, None);
        assert_eq!(p.timeout_ms, 9000.0);
        assert_eq!(p.port_range, 100.0);
        assert_eq!(p.send_min_rounds, 0.0);

        // PREDICTION_OFF 整包换成 HARDSYM：portRange min(500*2,500)=500、timeout 不 +4000
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::PREDICTION_OFF),
            None,
        );
        assert_eq!(p.port_range, 500.0);
        assert_eq!(p.timeout_ms, 30000.0);
        assert_eq!(p.send_interval, 150.0);
        assert!(p.hard_sym_spray);
        assert!(!p.easy_sym_bomb);
        assert_eq!(p.send_min_rounds, 3.0);
        assert_eq!(p.send_min_pass, 3.0);

        // RESPONSE_NO_ACK：trunc(200/2)=100；FAST_LANE 100/2=50 恰落下限；再小也被夹住
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::RESPONSE_NO_ACK),
            None,
        );
        assert_eq!(p.send_interval, 100.0);
        assert_eq!(p.send_min_pass, 3.0);
        assert_eq!(p.send_min_rounds, 0.0);
        let p = next_params(
            &FAST_LANE,
            0.0,
            8.0,
            Some(FailureReason::RESPONSE_NO_ACK),
            None,
        );
        assert_eq!(p.send_interval, 50.0);
        assert_eq!(p.port_range, 20.0);

        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::FIREWALL_DETECTED),
            None,
        );
        assert!(p.skip_direct_punch);
        assert_eq!(p.port_range, 100.0);
        assert_eq!(p.timeout_ms, 12000.0);

        let hit = result(1.0, 1.0, 1.0, 100.0, 0.0, false, false, true);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::PARTIAL_SUCCESS),
            Some(&hit),
        );
        assert!(p.reuse_successful_sockets);
        assert_eq!(p.successful_port_range, Some(vec![95.0, 105.0]));
        let zero = result(1.0, 1.0, 1.0, 0.0, 0.0, false, false, true);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::PARTIAL_SUCCESS),
            Some(&zero),
        );
        assert!(!p.reuse_successful_sockets);
        assert_eq!(p.successful_port_range, None);
        let half = result(1.0, 1.0, 1.0, 0.5, 0.0, false, false, true);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::PARTIAL_SUCCESS),
            Some(&half),
        );
        assert_eq!(p.successful_port_range, Some(vec![-4.5, 5.5]));

        let p = next_params(&DEFAULT, 0.0, 8.0, Some(FailureReason::ACK_TIMEOUT), None);
        assert_eq!(p.ack_retries, 3.0);
        assert_eq!(p.send_min_pass, 3.0);
        assert_eq!(p.send_min_rounds, 0.0);

        // elapsed>10000：timeout=min(trunc(16000*3/2),30000)=24000
        let slow = result(1.0, 0.0, 0.0, 0.0, 12000.0, false, false, false);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::NO_RESPONSE),
            Some(&slow),
        );
        assert_eq!(p.timeout_ms, 24000.0);
        assert_eq!(p.port_range, 200.0);
        // elapsed<3000 且仅 NO_RESPONSE：portRange 连乘两次 100→200→400
        let fast = result(1.0, 0.0, 0.0, 0.0, 2000.0, false, false, false);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::NO_RESPONSE),
            Some(&fast),
        );
        assert_eq!(p.port_range, 400.0);
        assert_eq!(p.timeout_ms, 16000.0);
        let p = next_params(
            &HARDSYM,
            0.0,
            8.0,
            Some(FailureReason::NO_RESPONSE),
            Some(&fast),
        );
        assert_eq!(p.port_range, 500.0);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::ACK_TIMEOUT),
            Some(&fast),
        );
        assert_eq!(p.port_range, 100.0);
        // HARDSYM 30000*3/2=45000 夹到 30000
        let p = next_params(
            &HARDSYM,
            0.0,
            8.0,
            Some(FailureReason::NO_RESPONSE),
            Some(&slow),
        );
        assert_eq!(p.timeout_ms, 30000.0);
        assert_eq!(p.port_range, 500.0);
        // elapsed=0 不进自适应
        let idle = result(1.0, 0.0, 0.0, 0.0, 0.0, false, false, false);
        let p = next_params(
            &DEFAULT,
            0.0,
            8.0,
            Some(FailureReason::NO_RESPONSE),
            Some(&idle),
        );
        assert_eq!(p.port_range, 200.0);
        assert_eq!(p.timeout_ms, 16000.0);
    }

    // 区分：8 个模板各自的 11 个入参字段（含 hardSymSpray/easySymBomb 两个身份位）
    #[test]
    fn from_profile_goldens() {
        let p = from_profile(&DEFAULT);
        assert_eq!(
            p,
            PunchParams {
                port_range: 100.0,
                timeout_ms: 12000.0,
                send_interval: 200.0,
                ack_retries: 1.0,
                skip_direct_punch: false,
                reuse_successful_sockets: false,
                successful_port_range: None,
                send_min_rounds: 0.0,
                send_min_pass: 0.0,
                hard_sym_spray: false,
                easy_sym_bomb: false,
                spray_port_count_min: 600.0,
                spray_port_count_max: 800.0,
                spray_packets_per_port: 3.0,
                spray_port_interval_ms: 1.0,
                spray_decay_numerator: 2.0,
                spray_decay_floor: 180.0,
                bomb_window: 20.0,
                bomb_round_interval_ms: 100.0,
                bomb_duration_ms: 5000.0,
            }
        );
        let aggressive = from_profile(&AGGRESSIVE);
        assert_eq!(aggressive.timeout_ms, 20000.0);
        assert_eq!(aggressive.send_interval, 200.0);
        assert_eq!(aggressive.port_range, 100.0);
        assert!(!aggressive.hard_sym_spray);
        let hardsym = from_profile(&HARDSYM);
        assert_eq!(hardsym.port_range, 500.0);
        assert_eq!(hardsym.timeout_ms, 30000.0);
        assert_eq!(hardsym.send_interval, 150.0);
        assert!(hardsym.hard_sym_spray);
        assert!(!hardsym.easy_sym_bomb);
        let dual = from_profile(&EASY_SYM_DUAL);
        assert_eq!(dual.port_range, 50.0);
        assert_eq!(dual.timeout_ms, 12000.0);
        assert!(dual.easy_sym_bomb);
        assert!(!dual.hard_sym_spray);
        let v100 = from_profile(&V100);
        assert_eq!(v100.port_range, 100.0);
        assert_eq!(v100.timeout_ms, 8000.0);
        assert_eq!(v100.send_interval, 200.0);
        assert!(!v100.hard_sym_spray);
        assert!(!v100.easy_sym_bomb);
        let fast = from_profile(&FAST_LANE);
        assert_eq!(fast.port_range, 20.0);
        assert_eq!(fast.timeout_ms, 6000.0);
        assert_eq!(fast.send_interval, 100.0);
    }

    // 区分：n*xx-x*x 的正规方程外推（slope*n + 截距均按 n 归一）、空/单样本短路、
    // 常数序列的 round(y/n)、非整 slope 的 Math.round 方向
    #[test]
    fn linear_regression_goldens() {
        assert_eq!(linear_regression_predict(&[]), -1.0);
        assert_eq!(linear_regression_predict(&[4400.0]), 4400.0);
        assert_eq!(linear_regression_predict(&[1000.0, 1100.0, 1200.0]), 1300.0);
        assert_eq!(linear_regression_predict(&[1000.0, 1000.0, 1000.0]), 1000.0);
        assert_eq!(linear_regression_predict(&[5000.0, 5000.0, 5000.0]), 5000.0);
        assert_eq!(linear_regression_predict(&[1000.0, 1100.0]), 1200.0);
        assert_eq!(
            linear_regression_predict(&[25565.0, 25565.0, 25566.0]),
            25566.0
        );
        assert_eq!(linear_regression_predict(&[1000.0, 900.0, 800.0]), 700.0);
    }

    // 区分：四分位修剪（trim=trunc(len/4)）、先排序后修剪、EMA α=0.4 的迭代次序、末端口锚点
    #[test]
    fn delta_predict_goldens() {
        assert_eq!(delta_predict(&[]), -1.0);
        assert_eq!(delta_predict(&[1000.0]), -1.0);
        assert_eq!(delta_predict(&[1000.0, 1100.0, 1200.0]), 1300.0);
        assert_eq!(
            delta_predict(&[1000.0, 1010.0, 1020.0, 1030.0, 1040.0]),
            1050.0
        );
        // 排序后 [-50,10,10,100] 修剪成 [10,10] → 1070+10；不排序或漏修剪会得 1044 / 1103
        assert_eq!(
            delta_predict(&[1000.0, 1100.0, 1050.0, 1060.0, 1070.0]),
            1080.0
        );
        assert_eq!(delta_predict(&[1000.0, 1000.0]), 1000.0);
        assert_eq!(
            delta_predict(&[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]),
            110.0
        );
    }

    // 区分：两支都正才加权、range 三档阈值的 >=、65535/1024 双向夹取、
    // 样本短路分支不夹取且 ports[0] ?? -1
    #[test]
    fn predict_goldens() {
        let p = predict(&[]);
        assert_eq!(p.predicted_port, -1.0);
        assert_eq!(p.range, 200.0);
        assert_eq!(p.strategy, "no_samples");
        let p = predict(&[1100.0]);
        assert_eq!(p.predicted_port, 1100.0);
        assert_eq!(p.range, 200.0);
        assert_eq!(p.strategy, "single_sample");
        let p = predict(&[500.0]);
        assert_eq!(p.predicted_port, 500.0);
        let p = predict(&[1000.0, 1100.0, 1200.0]);
        assert_eq!(p.predicted_port, 1300.0);
        assert_eq!(p.range, 100.0);
        assert_eq!(p.strategy, "combined");
        let p = predict(&[1000.0, 1100.0]);
        assert_eq!(p.predicted_port, 1200.0);
        assert_eq!(p.range, 200.0);
        let p = predict(&[1000.0, 1050.0, 1100.0, 1150.0]);
        assert_eq!(p.range, 100.0);
        let p = predict(&[1000.0, 1025.0, 1050.0, 1075.0, 1100.0]);
        assert_eq!(p.range, 64.0);
        let p = predict(&[
            1000.0, 1020.0, 1040.0, 1060.0, 1080.0, 1100.0, 1120.0, 1140.0, 1160.0,
        ]);
        assert_eq!(p.range, 64.0);
        let p = predict(&[
            1000.0, 1020.0, 1040.0, 1060.0, 1080.0, 1100.0, 1120.0, 1140.0, 1160.0, 1180.0,
        ]);
        assert_eq!(p.range, 32.0);
        let p = predict(&[70000.0, 70100.0, 70200.0]);
        assert_eq!(p.predicted_port, 65535.0);
        let p = predict(&[1000.0, 900.0, 800.0]);
        assert_eq!(p.predicted_port, 1024.0);
        let p = predict(&[0.0, 0.0, 0.0]);
        assert_eq!(p.predicted_port, 1024.0);
        assert_eq!(p.strategy, "combined");
    }

    // 区分：首元素恒为基准端口、每轮 +offset 先于 -offset、lo/hi 两侧夹取、
    // range=0/负数只出一个、range 大于 hi-lo 时上侧仍逐格铺到 hi
    #[test]
    fn generate_target_ports_goldens() {
        assert_eq!(
            generate_target_ports(25565.0, 3.0),
            vec![25565.0, 25566.0, 25564.0, 25567.0, 25563.0, 25568.0, 25562.0]
        );
        assert_eq!(generate_target_ports(25565.0, 0.0), vec![25565.0]);
        assert_eq!(generate_target_ports(25565.0, -1.0), vec![25565.0]);
        assert_eq!(
            generate_target_ports(1024.0, 2.0),
            vec![1024.0, 1025.0, 1026.0]
        );
        assert_eq!(
            generate_target_ports(65535.0, 2.0),
            vec![65535.0, 65534.0, 65533.0]
        );
        let wide = generate_target_ports(10.0, 1000.0);
        assert_eq!(wide.len(), 1001);
        assert_eq!(wide.first(), Some(&10.0));
        assert_eq!(wide.last(), Some(&1010.0));
        assert_eq!(wide[1], 11.0);
        // 下探一侧被 lo 全部挡住（10-offset < 1024），上探一侧仍逐格铺到 hi
        assert_eq!(
            generate_target_ports(1025.0, 2.0),
            vec![1025.0, 1026.0, 1024.0, 1027.0]
        );
    }

    // 区分：重试延迟 punchRetryDelayMs*2^(attempt-1) 的自增时机（attempt++）、
    // attempt<punchMaxAttempts 的边界、跨轮后回退 BACKOFF_DELAYS_MS、轮末返回 0 并换模板、
    // 换模板后的 20 秒节流令第二次轮末切换失效
    #[test]
    fn retry_and_backoff() {
        let mut rounds = PunchRounds::default();
        assert_eq!(rounds.max_cycles(), 3.0);
        assert_eq!(rounds.advance(false), 800.0);
        assert_eq!(rounds.attempt, 2.0);
        assert_eq!(rounds.advance(false), 1600.0);
        assert_eq!(rounds.attempt, 3.0);
        assert_eq!(rounds.advance(false), 1000.0);
        assert_eq!(rounds.cycle, 1.0);
        assert_eq!(rounds.attempt, 1.0);
        assert_eq!(rounds.advance(false), 800.0);
        assert_eq!(rounds.advance(true), 2000.0);
        assert_eq!(rounds.cycle, 2.0);
        assert_eq!(rounds.advance(true), 0.0);
        assert_eq!(rounds.cycle, 0.0);
        assert_eq!(rounds.round, 1.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
        assert_eq!(rounds.advance(true), 1000.0);
        assert_eq!(rounds.advance(true), 2000.0);
        assert_eq!(rounds.advance(true), 0.0);
        assert_eq!(rounds.round, 2.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
        assert_eq!(rounds.advance(true), 1000.0);

        let mut hard = PunchRounds::default();
        hard.classify(NatClass::EASY_SYM, NatClass::HARD_SYM, 0.0);
        assert_eq!(name(&hard.profile), "HARDSYM");
        assert_eq!(hard.max_cycles(), 3.0);
        assert_eq!(hard.advance(false), 500.0);
        assert_eq!(hard.advance(false), 1000.0);
        assert_eq!(hard.advance(false), 2000.0);
        assert_eq!(hard.advance(false), 4000.0);
        assert_eq!(hard.attempt, 5.0);
        assert_eq!(hard.advance(false), 1000.0);
        assert_eq!(hard.attempt, 1.0);
        assert_eq!(hard.advance(false), 500.0);
        assert_eq!(hard.cycle, 1.0);
        assert_eq!(hard.round, 0.0);

        let mut aggressive = PunchRounds::default();
        aggressive.classify(NatClass::UNKNOWN, NatClass::CONE, 0.0);
        assert_eq!(name(&aggressive.profile), "AGGRESSIVE");
        assert_eq!(aggressive.max_cycles(), 3.0);
        assert_eq!(aggressive.advance(false), 600.0);
        assert_eq!(aggressive.advance(false), 1200.0);
        assert_eq!(aggressive.advance(false), 2400.0);
        assert_eq!(aggressive.advance(false), 1000.0);
        assert_eq!(aggressive.advance(false), 600.0);
        assert_eq!(aggressive.cycle, 1.0);
    }

    // 区分：BACKOFF_DELAYS_MS 用 min(oldCycle, len-1) 夹表尾（第 3 个 4000 之后不再增长），
    // 且只有 cycle>=maxCycles 才进 round
    #[test]
    fn backoff_table_tail_is_clamped() {
        let mut wide = PunchRounds::default();
        wide.reachable = 6.0;
        assert_eq!(wide.max_cycles(), 6.0);
        assert_eq!(wide.advance(true), 1000.0);
        assert_eq!(wide.advance(true), 2000.0);
        assert_eq!(wide.advance(true), 4000.0);
        assert_eq!(wide.advance(true), 4000.0);
        assert_eq!(wide.advance(true), 4000.0);
        assert_eq!(wide.cycle, 5.0);
        assert_eq!(wide.round, 0.0);
        assert_eq!(name(&wide.profile), "DEFAULT");
        assert_eq!(wide.advance(true), 0.0);
        assert_eq!(wide.cycle, 0.0);
        assert_eq!(wide.round, 1.0);
    }

    // 区分：连续三次同因升级发生在轮末 return 0 之后（本轮仍返回表值）、NO_RESPONSE 走 HARDSYM
    // 但被前一次切换的 20 秒节流挡住、consecutive 命中后归零
    #[test]
    fn consecutive_failure_escalation() {
        let mut rounds = PunchRounds::default();
        rounds.latest_failure = Some(FailureReason::NO_RESPONSE);
        assert_eq!(rounds.advance(true), 1000.0);
        assert_eq!(name(&rounds.profile), "DEFAULT");
        assert_eq!(rounds.consecutive, 1.0);
        assert_eq!(rounds.advance(true), 2000.0);
        assert_eq!(rounds.consecutive, 2.0);
        assert_eq!(rounds.advance(true), 0.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
        assert_eq!(rounds.consecutive, 2.0);
        assert_eq!(rounds.advance(true), 1000.0);
        assert_eq!(rounds.consecutive, 0.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
        assert_eq!(rounds.last_failure, Some(FailureReason::NO_RESPONSE));
    }

    // 区分：cycle=0 + DEFAULT + 任一侧对称 → 立刻升级 AGGRESSIVE（在回退表之前）
    #[test]
    fn symmetric_local_upgrades_on_first_cycle() {
        let mut rounds = PunchRounds::default();
        rounds.local = NatClass::EASY_SYM;
        assert_eq!(rounds.advance(true), 1000.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
        assert_eq!(rounds.cycle, 1.0);
        assert_eq!(rounds.round, 0.0);
        assert_eq!(rounds.advance(true), 2000.0);
        assert_eq!(name(&rounds.profile), "AGGRESSIVE");
    }

    // 区分：maxCycles 的 reachable 真值短路、对称/非对称取 maxSymCycles 与 maxCycles、
    // Math.max(1|2, …) 的下限、classify 换 NAT 时清 params、terminal 的两个 >=
    #[test]
    fn rounds_state_machine() {
        let mut rounds = PunchRounds::default();
        assert_eq!(rounds.max_cycles(), 3.0);
        rounds.reachable = 5.0;
        assert_eq!(rounds.max_cycles(), 5.0);
        rounds.reachable = 0.5;
        assert_eq!(rounds.max_cycles(), 1.0);
        rounds.classify(NatClass::HARD_SYM, NatClass::HARD_SYM, 0.5);
        assert_eq!(name(&rounds.profile), "HARDSYM");
        assert_eq!(rounds.max_cycles(), 2.0);
        rounds.reachable = 9.0;
        assert_eq!(rounds.max_cycles(), 9.0);

        let mut fresh = PunchRounds::default();
        fresh.classify(NatClass::CONE, NatClass::CONE, 0.0);
        assert_eq!(name(&fresh.profile), "FAST_LANE");
        fresh.params = Some(from_profile(&DEFAULT));
        fresh.classify(NatClass::CONE, NatClass::HARD_SYM, 0.0);
        assert_eq!(fresh.params, None);
        assert_eq!(name(&fresh.profile), "FAST_LANE");
        fresh.remote = NatClass::CONE;
        fresh.reachable = 9.0;
        assert_eq!(fresh.max_cycles(), 8.0);

        let mut terminal = PunchRounds::default();
        assert!(!terminal.terminal());
        terminal.round = 19.0;
        assert!(!terminal.terminal());
        terminal.round = 20.0;
        assert!(terminal.terminal());
        let mut capped = PunchRounds::default();
        capped.prediction_off_count = 49.0;
        assert!(!capped.terminal());
        capped.prediction_off_count = 50.0;
        assert!(capped.terminal());
        let mut got = PunchRounds::default();
        got.round = 99.0;
        got.received_ever = true;
        assert!(!got.terminal());
    }

    // 区分：record 的 receivedEver 两条来源、PREDICTION_OFF 计数、
    // nextParams 传的是 cycle+1（不是 cycle）与当时的 maxCycles
    #[test]
    fn record_goldens() {
        let mut rounds = PunchRounds::default();
        rounds.classify(NatClass::EASY_SYM, NatClass::HARD_SYM, 0.0);
        assert_eq!(name(&rounds.profile), "HARDSYM");
        let miss = result(1.0, 0.0, 0.0, 150.0, 0.0, false, false, false);
        assert_eq!(rounds.record(&miss), FailureReason::PREDICTION_OFF);
        assert_eq!(rounds.prediction_off_count, 1.0);
        assert!(!rounds.received_ever);
        let params = rounds.params.clone().expect("params 应已写入");
        assert_eq!(params.port_range, 500.0);
        assert_eq!(params.timeout_ms, 22500.0);
        assert_eq!(params.send_min_pass, 3.0);
        assert!(params.hard_sym_spray);

        let mut other = PunchRounds::default();
        let ack_only = result(2.0, 0.0, 1.0, 0.0, 500.0, false, false, false);
        assert_eq!(other.record(&ack_only), FailureReason::ACK_TIMEOUT);
        assert!(other.received_ever);
        assert_eq!(other.prediction_off_count, 0.0);
        let params = other.params.clone().expect("params 应已写入");
        assert_eq!(params.ack_retries, 3.0);
        assert_eq!(params.send_min_pass, 3.0);
        assert_eq!(params.send_min_rounds, 0.0);
        assert_eq!(params.port_range, 100.0);
        assert_eq!(params.timeout_ms, 9000.0);
    }

    // 区分：camelCase 键名、键顺序（决定 JSON.stringify 字节序）、整数不带 .0、
    // successfulPortRange 的 null/数组形状
    #[test]
    fn json_wire_shapes() {
        assert_eq!(
            serde_json::to_string(&from_profile(&DEFAULT)).unwrap(),
            "{\"portRange\":100,\"timeoutMs\":12000,\"sendInterval\":200,\"ackRetries\":1,\
             \"skipDirectPunch\":false,\"reuseSuccessfulSockets\":false,\"successfulPortRange\":null,\
             \"sendMinRounds\":0,\"sendMinPass\":0,\"hardSymSpray\":false,\"easySymBomb\":false,\
             \"sprayPortCountMin\":600,\"sprayPortCountMax\":800,\"sprayPacketsPerPort\":3,\
             \"sprayPortIntervalMs\":1,\"sprayDecayNumerator\":2,\"sprayDecayFloor\":180,\
             \"bombWindow\":20,\"bombRoundIntervalMs\":100,\"bombDurationMs\":5000}"
        );
        let mut params = from_profile(&DEFAULT);
        params.successful_port_range = Some(vec![95.0, 105.0]);
        let text = serde_json::to_string(&params).unwrap();
        assert!(
            text.contains("\"successfulPortRange\":[95,105]"),
            "{}",
            text
        );
        let back: PunchParams = serde_json::from_str(&text).unwrap();
        assert_eq!(back, params);
        assert_eq!(
            serde_json::to_string(&predict(&[1000.0, 1100.0, 1200.0])).unwrap(),
            "{\"predictedPort\":1300,\"range\":100,\"strategy\":\"combined\"}"
        );
        assert_eq!(
            serde_json::to_string(&PunchResult {
                sockets_tried: 2.0,
                sockets_received_punch: 1.0,
                sockets_received_ack: 1.0,
                prediction_delta: 0.5,
                elapsed_ms: 0.0,
                firewall_detected: false,
                port_prediction_active: true,
                success: true,
            })
            .unwrap(),
            "{\"socketsTried\":2,\"socketsReceivedPunch\":1,\"socketsReceivedAck\":1,\
             \"predictionDelta\":0.5,\"elapsedMs\":0,\"firewallDetected\":false,\
             \"portPredictionActive\":true,\"success\":true}"
        );
        let parsed: FailureReason = serde_json::from_str("\"PREDICTION_OFF\"").unwrap();
        assert_eq!(parsed, FailureReason::PREDICTION_OFF);
        let nat: NatClass = serde_json::from_str("\"EASY_SYM\"").unwrap();
        assert_eq!(nat, NatClass::EASY_SYM);
        let strategy: PunchStrategy = serde_json::from_str("\"REVERSE_THEN_FORWARD\"").unwrap();
        assert_eq!(strategy, PunchStrategy::REVERSE_THEN_FORWARD);
        // 区分：入站整数字面量进 f64 字段、缺字段按原版一样直接报错（不静默补默认值）
        let wired: PunchResult = serde_json::from_str(
            "{\"socketsTried\":1,\"socketsReceivedPunch\":0,\"socketsReceivedAck\":0,\
             \"predictionDelta\":2,\"elapsedMs\":1,\"firewallDetected\":false,\
             \"portPredictionActive\":false,\"success\":false}",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(wired.prediction_delta, 2.0);
        assert_eq!(classify_failure(Some(&wired)), FailureReason::NO_RESPONSE);
        let partial: Result<PunchResult, _> =
            serde_json::from_str("{\"socketsTried\":1,\"success\":false}");
        assert!(partial.is_err());
        let params_partial: Result<PunchParams, _> =
            serde_json::from_str("{\"portRange\":100,\"timeoutMs\":12000}");
        assert!(params_partial.is_err());
    }

    // 区分：TUNER/PREDICTOR 的 SCREAMING_SNAKE 键名与全部数值
    #[test]
    fn tuner_and_predictor_constants() {
        assert_eq!(
            serde_json::to_string(&TUNER).unwrap(),
            "{\"MAX_PORT_RANGE\":500,\"MAX_TIMEOUT_MS\":30000,\"MIN_SEND_INTERVAL_MS\":50,\
             \"LATE_CYCLE_TIMEOUT_MS\":5000,\"ACK_RETRIES_ON_TIMEOUT\":3,\
             \"PREDICTION_DELTA_THRESHOLD\":100,\"PORT_RANGE_MULTIPLIER\":2,\
             \"TIMEOUT_INCREMENT_MS\":4000,\"SEND_INTERVAL_DIVISOR\":2}"
        );
        assert_eq!(
            serde_json::to_string(&PREDICTOR).unwrap(),
            "{\"SAMPLES_HIGH\":10,\"RANGE_HIGH\":32,\"SAMPLES_MID\":5,\"RANGE_MID\":64,\
             \"SAMPLES_LOW\":3,\"RANGE_LOW\":100,\"RANGE_DEFAULT\":200,\"EPSILON\":1e-9,\
             \"ALPHA\":0.4,\"LR_WEIGHT\":0.6,\"DELTA_WEIGHT\":0.4,\"MIN_PORT\":1024,\
             \"MAX_PORT\":65535}"
        );
        assert_eq!(ZERO_RECV_FINAL_ROUND_LIMIT, 20.0);
        assert_eq!(PREDICTION_OFF_CAP, 50.0);
        assert_eq!(BACKOFF_DELAYS_MS, [1000.0, 2000.0, 4000.0]);
        assert_eq!(PROFILE_SWITCH_COOLDOWN_MS, 20000.0);
    }

    // 区分：Math.round 是 floor(x+0.5)（-2.5→-2 而非 -3）、Math.trunc 朝零取整
    #[test]
    fn rounding_helpers() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(-2.4999), -2.0);
        assert_eq!(js_round(1.4), 1.0);
        assert_eq!((25.0f64 / 2.0f64).trunc(), 12.0f64);
        assert_eq!(js_min(500.0, 1000.0), 500.0);
        assert_eq!(js_number_text(19999.5), "19999.5");
        assert_eq!(js_number_text(20000.0), "20000");
        assert!(!js_truthy(f64::NAN));
        assert!(!js_truthy(-0.0));
        assert!(js_truthy(0.5));
    }

    // 区分：DEFAULT→AGGRESSIVE 的差异集只含变更键、未变更键被剔除
    #[test]
    fn profile_differences_shape() {
        let text = profile_differences(&AGGRESSIVE);
        let value: Value = serde_json::from_str(&text).unwrap();
        let entries = value.as_object().unwrap();
        assert_eq!(entries.get("name"), Some(&Value::from("AGGRESSIVE")));
        assert_eq!(entries.get("punchTimeoutMs"), Some(&Value::from(20000)));
        assert_eq!(entries.get("hostRoundTimeoutMs"), Some(&Value::from(10000)));
        assert!(!entries.contains_key("punchTimeout"));
        assert!(!entries.contains_key("portPredictionMaxRange"));
        let same = profile_differences(&DEFAULT);
        assert_eq!(same, "{}");
    }
}
