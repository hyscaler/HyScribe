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

#[cfg(test)]
mod tests {
    use hyscribe_model::hooks::{apply_filters, do_action};

    use super::*;

    #[test]
    fn records_and_answers_every_hook() {
        let answers = [("answered", Value::Int(2))];
        let (got, fired) = recorded(&answers, || {
            do_action("act", &[&1_i64]);
            (
                apply_filters("answered", 1_i64, &[]),
                apply_filters("kept", 3_i64, &[]),
            )
        });
        assert_eq!(got, (2, 3));
        let expected = [
            ("act".to_string(), Value::Null, vec![Value::Int(1)]),
            ("answered".to_string(), Value::Int(1), vec![]),
            ("kept".to_string(), Value::Int(3), vec![]),
        ];
        assert_eq!(fired, expected);
    }
}
