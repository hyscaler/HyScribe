// SPDX-License-Identifier: AGPL-3.0-or-later

//! The writes of `wp_insert_term()`, `wp_update_term()`,
//! `wp_delete_term()`, `wp_set_object_terms()`, `wp_remove_object_terms()`
//! and the term counts.

use super::{Query, Table, Write};
use crate::{Bind, Columns, DbError, Row, Sql, db_error, ids_or_none, list};

/// A term in a taxonomy, as the term functions read it inside an action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermRow {
    /// `term_id`.
    pub term_id: u64,
    /// `name`.
    pub name: Vec<u8>,
    /// `slug`.
    pub slug: Vec<u8>,
    /// `term_group`.
    pub term_group: i64,
    /// `term_taxonomy_id`.
    pub term_taxonomy_id: u64,
    /// `description`.
    pub description: Vec<u8>,
    /// `parent`.
    pub parent: u64,
    /// `count`, as stored.
    pub count: i64,
}

fn term_row(row: &Row) -> Result<TermRow, DbError> {
    let mut c = Columns::new(row);
    let term = TermRow {
        term_id: c.unsigned("term_id"),
        name: c.bytes("name"),
        slug: c.bytes("slug"),
        term_group: c.signed("term_group"),
        term_taxonomy_id: c.unsigned("term_taxonomy_id"),
        description: c.bytes("description"),
        parent: c.unsigned("parent"),
        count: c.signed("count"),
    };
    c.finish(term)
}

/// Two unsigned columns of a row.
fn pair(row: &Row, first: &str, second: &str) -> Result<(u64, u64), DbError> {
    let mut c = Columns::new(row);
    let pair = (c.unsigned(first), c.unsigned(second));
    c.finish(pair)
}

/// The query of [`TermRow`]s where `$where`, joined as `WP_Term_Query`
/// joins them.
macro_rules! terms_where {
    ($where:literal) => {
        concat!(
            "SELECT t.term_id, t.name, t.slug, t.term_group, tt.term_taxonomy_id, \
             tt.description, tt.parent, tt.count FROM {prefix}terms AS t INNER JOIN \
             {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id WHERE ",
            $where
        )
    };
}

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

    // The reads of the term functions, inside the action. Names and slugs
    // are text, so they compare under the column's collation, as in PHP.

    async fn term_rows(&mut self, query: Query) -> Result<Vec<TermRow>, DbError> {
        self.spend()?;
        let rows = query.fetch_all(&mut *self.tx).await.map_err(db_error)?;
        rows.iter().map(term_row).collect()
    }

    async fn pairs(
        &mut self,
        query: Query,
        (first, second): (&str, &str),
    ) -> Result<Vec<(u64, u64)>, DbError> {
        self.spend()?;
        let rows = query.fetch_all(&mut *self.tx).await.map_err(db_error)?;
        rows.iter().map(|row| pair(row, first, second)).collect()
    }

    /// `get_term()`: term `id` in `taxonomy`.
    pub async fn term_in(&mut self, taxonomy: &str, id: u64) -> Result<Option<TermRow>, DbError> {
        let query = self.sql(terms_where!("tt.taxonomy = ? AND t.term_id = ? LIMIT 1"));
        let rows = self.term_rows(query.bind(taxonomy).bind(id)).await?;
        Ok(rows.into_iter().next())
    }

    /// The first term of `taxonomy` by ID with `slug`, or of all for
    /// `None`, as `get_terms()` drops an empty slug text.
    pub async fn term_by_slug(
        &mut self,
        taxonomy: &str,
        slug: Option<&str>,
    ) -> Result<Option<TermRow>, DbError> {
        let query = self.sql(terms_where!(
            "tt.taxonomy = ? AND (? OR t.slug = ?) ORDER BY t.term_id LIMIT 1"
        ));
        let query = query.bind(taxonomy).bind(slug.is_none());
        let rows = self.term_rows(query.bind(slug.unwrap_or_default())).await?;
        Ok(rows.into_iter().next())
    }

    /// `term_exists()`'s query by name: the first term of `taxonomy` by ID
    /// named `name`.
    pub async fn term_by_name(
        &mut self,
        taxonomy: &str,
        name: &str,
    ) -> Result<Option<TermRow>, DbError> {
        let query = self.sql(terms_where!(
            "tt.taxonomy = ? AND t.name = ? ORDER BY t.term_id LIMIT 1"
        ));
        let rows = self.term_rows(query.bind(taxonomy).bind(name)).await?;
        Ok(rows.into_iter().next())
    }

    /// The terms of `taxonomy` under `parent` named `name`, or all of them
    /// for `None`, by name: `wp_insert_term()`'s name matches and siblings.
    pub async fn terms_under(
        &mut self,
        taxonomy: &str,
        parent: u64,
        name: Option<&str>,
    ) -> Result<Vec<TermRow>, DbError> {
        let query = self.sql(terms_where!(
            "tt.taxonomy = ? AND tt.parent = ? AND (? OR t.name = ?) ORDER BY t.name"
        ));
        let query = query.bind(taxonomy).bind(parent).bind(name.is_none());
        self.term_rows(query.bind(name.unwrap_or_default())).await
    }

    /// `wp_insert_term()`'s last check: a term of `taxonomy` under `parent`
    /// with `slug`, older than the new `(term_id, term_taxonomy_id)`.
    pub async fn older_duplicate(
        &mut self,
        (taxonomy, parent): (&str, u64),
        slug: &str,
        (term, term_taxonomy): (u64, u64),
    ) -> Result<Option<TermRow>, DbError> {
        let query = self.sql(terms_where!(
            "t.slug = ? AND tt.parent = ? AND tt.taxonomy = ? AND t.term_id < ? \
             AND tt.term_taxonomy_id != ? LIMIT 1"
        ));
        let query = query.bind(slug).bind(parent).bind(taxonomy);
        let rows = self.term_rows(query.bind(term).bind(term_taxonomy)).await?;
        Ok(rows.into_iter().next())
    }

    /// `term_exists( $id )`: whether term `id` is in a taxonomy.
    pub async fn term_exists(&mut self, id: u64) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}terms AS t INNER JOIN {prefix}term_taxonomy AS tt \
             ON t.term_id = tt.term_id WHERE t.term_id = ?",
        );
        Ok(self.count(query.bind(id)).await? > 0)
    }

    /// `term_exists( $text )`: whether a term of any taxonomy has `slug`,
    /// any slug for `None`, or `name`.
    pub async fn term_matches(&mut self, slug: Option<&str>, name: &str) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}terms AS t INNER JOIN {prefix}term_taxonomy AS tt \
             ON t.term_id = tt.term_id WHERE ? OR t.slug = ? OR t.name = ?",
        );
        let query = query.bind(slug.is_none()).bind(slug.unwrap_or_default());
        Ok(self.count(query.bind(name)).await? > 0)
    }

    /// `wp_unique_term_slug()`: whether a term but `except` has `slug`.
    pub async fn slug_taken(&mut self, slug: &str, except: u64) -> Result<bool, DbError> {
        let query = self.sql("SELECT COUNT(*) FROM {prefix}terms WHERE slug = ? AND term_id != ?");
        Ok(self.count(query.bind(slug).bind(except)).await? > 0)
    }

    /// `wp_get_object_terms()`: the terms of `taxonomy` that `object` has,
    /// as `(term_id, term_taxonomy_id)`.
    pub async fn object_terms(
        &mut self,
        object: u64,
        taxonomy: &str,
    ) -> Result<Vec<(u64, u64)>, DbError> {
        let query = self.sql(
            "SELECT t.term_id, tt.term_taxonomy_id FROM {prefix}terms AS t INNER JOIN \
             {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id INNER JOIN \
             {prefix}term_relationships AS tr ON tr.term_taxonomy_id = tt.term_taxonomy_id \
             WHERE tt.taxonomy = ? AND tr.object_id = ? ORDER BY tt.term_taxonomy_id",
        );
        let query = query.bind(taxonomy).bind(object);
        self.pairs(query, ("term_id", "term_taxonomy_id")).await
    }

    /// `wp_set_object_terms()`: whether `object` has the term taxonomy
    /// `term_taxonomy`.
    pub async fn has_relationship(
        &mut self,
        object: u64,
        term_taxonomy: u64,
    ) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}term_relationships WHERE object_id = ? \
             AND term_taxonomy_id = ?",
        );
        Ok(self.count(query.bind(object).bind(term_taxonomy)).await? > 0)
    }

    /// `wp_delete_term()`: the objects with the term taxonomy `id`.
    pub async fn term_object_ids(&mut self, id: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT object_id FROM {prefix}term_relationships WHERE term_taxonomy_id = ? \
             ORDER BY object_id",
        );
        self.ids(query.bind(id)).await
    }

    /// `wp_insert_term()`'s new group for an alias: the highest
    /// `term_group`, 0 without terms.
    pub async fn max_term_group(&mut self) -> Result<i64, DbError> {
        let query = self.sql("SELECT COALESCE(MAX(term_group), 0) FROM {prefix}terms");
        // WordPress never writes a negative group.
        Ok(self.count(query).await? as i64)
    }

    /// `_get_term_hierarchy()`: each term of `taxonomy` with its parent, by
    /// term ID.
    pub async fn term_parents(&mut self, taxonomy: &str) -> Result<Vec<(u64, u64)>, DbError> {
        let query = self.sql(
            "SELECT t.term_id, tt.parent FROM {prefix}terms AS t INNER JOIN \
             {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id WHERE tt.taxonomy = ? \
             ORDER BY t.term_id",
        );
        self.pairs(query.bind(taxonomy), ("term_id", "parent"))
            .await
    }

    /// `_wp_delete_tax_menu_item()`: how many menu items link to term `id`
    /// of `taxonomy`.
    pub async fn term_menu_items(&mut self, id: u64, taxonomy: &str) -> Result<u64, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}posts AS p \
             INNER JOIN {prefix}postmeta AS o ON o.post_id = p.ID \
             AND o.meta_key = '_menu_item_object_id' \
             INNER JOIN {prefix}postmeta AS k ON k.post_id = p.ID \
             AND k.meta_key = '_menu_item_type' \
             INNER JOIN {prefix}postmeta AS x ON x.post_id = p.ID \
             AND x.meta_key = '_menu_item_object' \
             WHERE p.post_type = 'nav_menu_item' AND o.meta_value = ? \
             AND k.meta_value = 'taxonomy' AND x.meta_value = ?",
        );
        self.count(query.bind(id.to_string()).bind(taxonomy)).await
    }
}
