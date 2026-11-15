// SPDX-License-Identifier: AGPL-3.0-or-later

//! An action's statements, run one after another until one fails. The
//! first error is kept and every later statement is skipped and reads as
//! empty, as hyscribe-http's reads do; the action then returns the error,
//! so its transaction is rolled back, never committed.

use hyscribe_db::Write;

pub(crate) struct Steps<'a, 'w, E> {
    write: &'a mut Write<'w>,
    error: Option<E>,
}

impl<'a, 'w, E> Steps<'a, 'w, E> {
    pub(crate) fn new(write: &'a mut Write<'w>) -> Self {
        Steps { write, error: None }
    }

    /// The statement's result, or its type's default once one has failed.
    pub(crate) async fn run<T: Default, F: Into<E>>(
        &mut self,
        statement: impl AsyncFnOnce(&mut Write<'w>) -> Result<T, F>,
    ) -> T {
        if self.error.is_some() {
            return T::default();
        }
        let result = statement(self.write).await;
        self.keep(result.map_err(Into::into))
    }

    /// The value, or its type's default with the error kept, unless an
    /// earlier one was.
    pub(crate) fn keep<T: Default>(&mut self, result: Result<T, E>) -> T {
        result.unwrap_or_else(|err| {
            self.error.get_or_insert(err);
            T::default()
        })
    }

    /// The action's result: `result`, or the first error.
    pub(crate) fn finish<T>(self, result: Result<T, E>) -> Result<T, E> {
        self.error.map_or(result, Err)
    }
}

/// Runs one statement of the action: `step!(steps, method(args))`.
macro_rules! step {
    ($steps:expr, $method:ident($($arg:expr),* $(,)?)) => {
        $steps
            .run(async |write| write.$method($($arg),*).await)
            .await
    };
}
pub(crate) use step;

#[cfg(test)]
mod tests;
