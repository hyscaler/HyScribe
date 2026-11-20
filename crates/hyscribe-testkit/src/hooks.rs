// SPDX-License-Identifier: AGPL-3.0-or-later

//! A dispatcher for the hook points' tests: it records each hook fired with
//! its value and arguments as PHP values, and answers a filter with the
//! value given for its hook, or the value it was handed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use hyscribe_model::hooks::{Dispatch, scoped};
use hyscribe_phpser::Value;

/// A hook fired: its name, its value (null for an action) and arguments.
pub type Fired = (String, Value, Vec<Value>);

/// Records the hooks fired and answers filters from its table.
struct Recorder {
    answers: BTreeMap<String, Value>,
    fired: Mutex<Vec<Fired>>,
}

impl Dispatch for Recorder {
    fn attached(&self, _: &str) -> bool {
        true
    }

    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value {
        let answer = self.answers.get(hook).cloned();
        let mut fired = self.fired.lock().unwrap_or_else(PoisonError::into_inner);
        fired.push((hook.into(), value.clone(), args.to_vec()));
        answer.unwrap_or(value)
    }

    fn action(&self, hook: &str, args: &[Value]) {
        self.filter(hook, Value::Null, args);
    }
}

/// Runs `run` with every hook attached, filters answered from `answers`,
/// and gives back what it returned and the hooks it fired, in order.
pub fn recorded<R>(answers: &[(&str, Value)], run: impl FnOnce() -> R) -> (R, Vec<Fired>) {
    let recorder = Arc::new(Recorder {
        answers: answers
            .iter()
            .map(|(hook, value)| ((*hook).into(), value.clone()))
            .collect(),
        fired: Mutex::new(Vec::new()),
    });
    let dispatch: Arc<dyn Dispatch> = recorder.clone();
    let ran = scoped(&dispatch, run);
    let fired = recorder
        .fired
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (ran, fired)
}
