use serde_json::Value;

use crate::document::Document;
use crate::error::NexDbResult;

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq(Value),
    Ne(Value),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    In(Vec<Value>),
    Between(Value, Value),
    Exists(bool),
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
}

#[derive(Debug, Clone)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub field: String,
    pub order: SortOrder,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub collection: String,
    pub filters: Vec<Filter>,
    pub sorts: Vec<Sort>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl Query {
    pub fn new(collection: impl Into<String>) -> Self {
        Query {
            collection: collection.into(),
            filters: Vec::new(),
            sorts: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn filter(mut self, field: impl Into<String>, op: FilterOp) -> Self {
        self.filters.push(Filter { field: field.into(), op });
        self
    }

    pub fn eq(mut self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filters.push(Filter {
            field: field.into(),
            op: FilterOp::Eq(value.into()),
        });
        self
    }

    pub fn gt(mut self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filters.push(Filter {
            field: field.into(),
            op: FilterOp::Gt(value.into()),
        });
        self
    }

    pub fn lt(mut self, field: impl Into<String>, value: impl Into<Value>) -> Self {
        self.filters.push(Filter {
            field: field.into(),
            op: FilterOp::Lt(value.into()),
        });
        self
    }

    pub fn between(mut self, field: impl Into<String>, low: impl Into<Value>, high: impl Into<Value>) -> Self {
        self.filters.push(Filter {
            field: field.into(),
            op: FilterOp::Between(low.into(), high.into()),
        });
        self
    }

    pub fn sort(mut self, field: impl Into<String>, order: SortOrder) -> Self {
        self.sorts.push(Sort { field: field.into(), order });
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn offset(mut self, n: usize) -> Self {
        self.offset = Some(n);
        self
    }

    pub fn matches(&self, doc: &Document) -> bool {
        self.filters.iter().all(|f| self.matches_filter(doc, f))
    }

    fn matches_filter(&self, doc: &Document, filter: &Filter) -> bool {
        let doc_val = doc.get_path(&filter.field);
        match &filter.op {
            FilterOp::Eq(expected) => doc_val.map_or(false, |v| v == expected),
            FilterOp::Ne(expected) => doc_val.map_or(true, |v| v != expected),
            FilterOp::Gt(expected) => doc_val.map_or(false, |v| compare_values(v, expected) == Some(std::cmp::Ordering::Greater)),
            FilterOp::Gte(expected) => doc_val.map_or(false, |v| compare_values(v, expected) != Some(std::cmp::Ordering::Less)),
            FilterOp::Lt(expected) => doc_val.map_or(false, |v| compare_values(v, expected) == Some(std::cmp::Ordering::Less)),
            FilterOp::Lte(expected) => doc_val.map_or(false, |v| compare_values(v, expected) != Some(std::cmp::Ordering::Greater)),
            FilterOp::In(values) => doc_val.map_or(false, |v| values.contains(v)),
            FilterOp::Between(low, high) => doc_val.map_or(false, |v| {
                compare_values(v, low) != Some(std::cmp::Ordering::Less)
                    && compare_values(v, high) != Some(std::cmp::Ordering::Greater)
            }),
            FilterOp::Exists(should_exist) => doc_val.is_some() == *should_exist,
        }
    }

    pub fn apply_to(&self, docs: Vec<(String, Document)>) -> Vec<(String, Document)> {
        let mut filtered: Vec<_> = docs.into_iter()
            .filter(|(_, doc)| self.matches(doc))
            .collect();

        if !self.sorts.is_empty() {
            filtered.sort_by(|a, b| {
                for sort in &self.sorts {
                    let va = a.1.get_path(&sort.field);
                    let vb = b.1.get_path(&sort.field);
                    let cmp = compare_values(va.unwrap_or(&Value::Null), vb.unwrap_or(&Value::Null));
                    if let Some(ord) = cmp {
                        if ord != std::cmp::Ordering::Equal {
                            return match sort.order {
                                SortOrder::Asc => ord,
                                SortOrder::Desc => ord.reverse(),
                            };
                        }
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        let offset = self.offset.unwrap_or(0);
        let limit = self.limit.unwrap_or(usize::MAX);

        filtered.into_iter().skip(offset).take(limit).collect()
    }
}

fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => {
            if let (Some(a), Some(b)) = (na.as_f64(), nb.as_f64()) {
                return a.partial_cmp(&b);
            }
            None
        }
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

pub fn parse_query_from_json(collection: &str, json: &Value) -> NexDbResult<Query> {
    let mut q = Query::new(collection);

    if let Some(filter_obj) = json.get("filter").and_then(|v| v.as_object()) {
        for (field, condition) in filter_obj {
            if let Some(obj) = condition.as_object() {
                for (op_key, val) in obj {
                    match op_key.as_str() {
                        "$eq" => { q = q.filter(field, FilterOp::Eq(val.clone())); }
                        "$ne" => { q = q.filter(field, FilterOp::Ne(val.clone())); }
                        "$gt" => { q = q.filter(field, FilterOp::Gt(val.clone())); }
                        "$gte" => { q = q.filter(field, FilterOp::Gte(val.clone())); }
                        "$lt" => { q = q.filter(field, FilterOp::Lt(val.clone())); }
                        "$lte" => { q = q.filter(field, FilterOp::Lte(val.clone())); }
                        "$in" => {
                            if let Some(arr) = val.as_array() {
                                q = q.filter(field, FilterOp::In(arr.clone()));
                            }
                        }
                        "$exists" => {
                            if let Some(b) = val.as_bool() {
                                q = q.filter(field, FilterOp::Exists(b));
                            }
                        }
                        "$between" => {
                            if let Some(arr) = val.as_array() {
                                if arr.len() == 2 {
                                    q = q.filter(field, FilterOp::Between(arr[0].clone(), arr[1].clone()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                q = q.filter(field, FilterOp::Eq(condition.clone()));
            }
        }
    }

    if let Some(sort_arr) = json.get("sort").and_then(|v| v.as_array()) {
        for entry in sort_arr {
            if let Some(obj) = entry.as_object() {
                for (field, dir) in obj {
                    let order = if dir.as_i64().unwrap_or(1) > 0 { SortOrder::Asc } else { SortOrder::Desc };
                    q = q.sort(field, order);
                }
            }
        }
    }

    if let Some(limit) = json.get("limit").and_then(|v| v.as_i64()) {
        q = q.limit(limit as usize);
    }

    if let Some(offset) = json.get("offset").and_then(|v| v.as_i64()) {
        q = q.offset(offset as usize);
    }

    Ok(q)
}
