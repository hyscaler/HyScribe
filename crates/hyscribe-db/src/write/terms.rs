// SPDX-License-Identifier: AGPL-3.0-or-later

//! The writes of `wp_insert_term()`, `wp_update_term()`,
//! `wp_delete_term()`, `wp_set_object_terms()`, `wp_remove_object_terms()`
//! and the term counts.

use super::{Table, Write};
use crate::dialect::Dialect;
use crate::{Bind, DbError, Engine, Sql, ids_or_none, list};

/// `values` as text, led by `''`, which keeps the list valid when empty and
/// matches no status or type.
fn texts_or_none<'a>(values: &'a [&str]) -> impl Iterator<Item = Bind> + 'a {
    std::iter::once("")
        .chain(values.iter().copied())
        .map(|value| Bind::Text(value.to_string()))
}

impl Write<'_> {
    /// `wp_insert_term()`'s `INSERT` into the terms: the new term's ID.
    pub async fn insert_term(
        &mut self,
        name: &[u8],
        slug: &[u8],
        group: i64,
    ) -> Result<u64, DbError> {
        let query = self
            .sql("INSERT INTO {prefix}terms (name, slug, term_group) VALUES (?, ?, ?)")
            .bind(name)
            .bind(slug)
            .bind(group);
        self.insert(Table::Terms, query).await
    }

    /// `wp_update_term()`'s `UPDATE` of the term.
    pub async fn update_term(
        &mut self,
        id: u64,
        name: &[u8],
        slug: &[u8],
        group: i64,
    ) -> Result<u64, DbError> {
        let query = self
            .sql("UPDATE {prefix}terms SET name = ?, slug = ?, term_group = ? WHERE term_id = ?")
            .bind(name)
            .bind(slug)
            .bind(group);
        self.change(Table::Terms, query.bind(id)).await
    }

    /// The slug `wp_insert_term()` and `wp_update_term()` set after the
    /// fact.
    pub async fn set_term_slug(&mut self, id: u64, slug: &[u8]) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}terms SET slug = ? WHERE term_id = ?");
        self.change(Table::Terms, query.bind(slug).bind(id)).await
    }

    /// The group `wp_insert_term()` gives the term an alias joins.
    pub async fn set_term_group(&mut self, id: u64, group: i64) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}terms SET term_group = ? WHERE term_id = ?");
        self.change(Table::Terms, query.bind(group).bind(id)).await
    }

    /// `wp_delete_term()`'s `DELETE` of a term no taxonomy uses.
    pub async fn delete_term(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}terms WHERE term_id = ?");
        self.change(Table::Terms, query.bind(id)).await
    }

    /// `wp_insert_term()`'s `INSERT` into the term taxonomies, with no
    /// objects: the new row's `term_taxonomy_id`.
    pub async fn insert_term_taxonomy(
        &mut self,
        (term, taxonomy): (u64, &str),
        description: &[u8],
        parent: u64,
    ) -> Result<u64, DbError> {
        let query = self.sql(
            "INSERT INTO {prefix}term_taxonomy (term_id, taxonomy, description, parent, count) \
             VALUES (?, ?, ?, ?, 0)",
        );
        let query = query
            .bind(term)
            .bind(taxonomy)
            .bind(description)
            .bind(parent);
        self.insert(Table::TermTaxonomy, query).await
    }

    /// `wp_update_term()`'s `UPDATE` of the term taxonomy `id`.
    pub async fn update_term_taxonomy(
        &mut self,
        id: u64,
        (term, taxonomy): (u64, &str),
        description: &[u8],
        parent: u64,
    ) -> Result<u64, DbError> {
        let query = self.sql(
            "UPDATE {prefix}term_taxonomy SET term_id = ?, taxonomy = ?, description = ?, \
             parent = ? WHERE term_taxonomy_id = ?",
        );
        let query = query
            .bind(term)
            .bind(taxonomy)
            .bind(description)
            .bind(parent);
        self.change(Table::TermTaxonomy, query.bind(id)).await
    }

    /// `wp_delete_term()`: the children in `taxonomy` of `parent` move to
    /// `new_parent`.
    pub async fn reparent_terms(
        &mut self,
        taxonomy: &str,
        parent: u64,
        new_parent: u64,
    ) -> Result<u64, DbError> {
        let query = self
            .sql("UPDATE {prefix}term_taxonomy SET parent = ? WHERE parent = ? AND taxonomy = ?");
        let query = query.bind(new_parent).bind(parent).bind(taxonomy);
        self.change(Table::TermTaxonomy, query).await
    }

    /// The term counts' `UPDATE`.
    pub async fn set_term_count(&mut self, id: u64, count: u64) -> Result<u64, DbError> {
        let query =
            self.sql("UPDATE {prefix}term_taxonomy SET count = ? WHERE term_taxonomy_id = ?");
        self.change(Table::TermTaxonomy, query.bind(count).bind(id))
            .await
    }

    /// `wp_delete_term()`'s `DELETE` of the term taxonomy `id`.
    pub async fn delete_term_taxonomy(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}term_taxonomy WHERE term_taxonomy_id = ?");
        self.change(Table::TermTaxonomy, query.bind(id)).await
    }

    /// How many taxonomies use term `id`, as `wp_delete_term()` counts them.
    pub async fn term_taxonomy_count(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("SELECT COUNT(*) FROM {prefix}term_taxonomy WHERE term_id = ?");
        self.count(query.bind(id)).await
    }

    /// `_update_post_term_count()`'s count of the posts of `types` in
    /// `statuses` with the term taxonomy `id`.
    pub async fn term_post_count(
        &mut self,
        id: u64,
        statuses: &[&str],
        types: &[&str],
    ) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(*) FROM {prefix}term_relationships, {prefix}posts \
             WHERE {prefix}posts.ID = {prefix}term_relationships.object_id AND post_status IN (",
        );
        list(&mut sql, texts_or_none(statuses));
        sql.push(" AND post_type IN (");
        list(&mut sql, texts_or_none(types));
        sql.push(" AND term_taxonomy_id = ?");
        sql.bind(Bind::Int(id));
        let query = sql.query(&self.writer.prefix);
        self.count(query).await
    }

    /// `_update_post_term_count()`'s count of the attachments with the term
    /// taxonomy `id` in `statuses`, or inheriting them from their parent.
    pub async fn term_attachment_count(
        &mut self,
        id: u64,
        statuses: &[&str],
    ) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(*) FROM {prefix}term_relationships, {prefix}posts p1 \
             WHERE p1.ID = {prefix}term_relationships.object_id AND ( post_status IN (",
        );
        list(&mut sql, texts_or_none(statuses));
        sql.push(
            " OR ( post_status = 'inherit' AND post_parent > 0 AND \
             ( SELECT post_status FROM {prefix}posts WHERE ID = p1.post_parent ) IN (",
        );
        list(&mut sql, texts_or_none(statuses));
        sql.push(" ) ) AND post_type = 'attachment' AND term_taxonomy_id = ?");
        sql.bind(Bind::Int(id));
        let query = sql.query(&self.writer.prefix);
        self.count(query).await
    }

    /// `_update_generic_term_count()`'s count of the objects with the term
    /// taxonomy `id`.
    pub async fn term_object_count(&mut self, id: u64) -> Result<u64, DbError> {
        let query =
            self.sql("SELECT COUNT(*) FROM {prefix}term_relationships WHERE term_taxonomy_id = ?");
        self.count(query.bind(id)).await
    }

    /// `wp_set_object_terms()`'s `INSERT` of one relationship.
    pub async fn add_relationship(
        &mut self,
        object: u64,
        term_taxonomy: u64,
    ) -> Result<u64, DbError> {
        let query = self.sql(
            "INSERT INTO {prefix}term_relationships (object_id, term_taxonomy_id) VALUES (?, ?)",
        );
        let query = query.bind(object).bind(term_taxonomy);
        self.change(Table::TermRelationships, query).await
    }

    /// `wp_set_object_terms()` for a sorted taxonomy: each `(term taxonomy,
    /// order)` of `orders` is written, or its order updated. `orders` must
    /// not be empty.
    pub async fn set_term_order(
        &mut self,
        object: u64,
        orders: &[(u64, u64)],
    ) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "INSERT INTO {prefix}term_relationships (object_id, term_taxonomy_id, term_order) VALUES ",
        );
        for (i, &(term_taxonomy, order)) in orders.iter().enumerate() {
            sql.push(if i == 0 { "(?, ?, ?)" } else { ", (?, ?, ?)" });
            for value in [object, term_taxonomy, order] {
                sql.bind(Bind::Int(value));
            }
        }
        Engine::insert_or_update(&mut sql, &["term_order"]);
        let query = sql.query(&self.writer.prefix);
        self.change(Table::TermRelationships, query).await
    }

    /// `wp_remove_object_terms()`'s `DELETE` of the relationships of
    /// `object` with the term taxonomies `term_taxonomies`.
    pub async fn remove_relationships(
        &mut self,
        object: u64,
        term_taxonomies: &[u64],
    ) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "DELETE FROM {prefix}term_relationships WHERE object_id = ? AND term_taxonomy_id IN (",
        );
        sql.bind(Bind::Int(object));
        list(&mut sql, ids_or_none(term_taxonomies));
        let query = sql.query(&self.writer.prefix);
        self.change(Table::TermRelationships, query).await
    }
}
