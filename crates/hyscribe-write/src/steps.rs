// SPDX-License-Identifier: AGPL-3.0-or-later

//! An action's statements, run one after another until one fails. The
//! first error is kept and every later statement is skipped and reads as
//! empty, as hyscribe-http's reads do; the action then returns the error,
//! so its transaction is rolled back, never committed.

use hyscribe_db::{DbError, Write};

pub(crate) struct Steps<'a, 'w> {
    write: &'a mut Write<'w>,
    error: Option<DbError>,
}

impl<'a, 'w> Steps<'a, 'w> {
    pub(crate) fn new(write: &'a mut Write<'w>) -> Self {
        Steps { write, error: None }
    }

    /// The statement's result, or its type's default once one has failed.
    pub(crate) async fn run<T: Default>(
        &mut self,
        statement: impl AsyncFnOnce(&mut Write<'w>) -> Result<T, DbError>,
    ) -> T {
        if self.error.is_some() {
            return T::default();
        }
        statement(self.write).await.unwrap_or_else(|err| {
            self.error = Some(err);
            T::default()
        })
    }

    /// The action's result: `value`, or the first error.
    pub(crate) fn finish<T>(self, value: T) -> Result<T, DbError> {
        self.error.map_or(Ok(value), Err)
    }
}

#[cfg(test)]
mod tests;
