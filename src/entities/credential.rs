use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "credentials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub selection_weight: i32,
    pub notes: Option<String>,
    pub upstream_base_url: Option<String>,
    pub account_id: Option<String>,
    pub account_email: Option<String>,
    pub plan_type: Option<String>,
    pub last_used_at: Option<DateTimeUtc>,
    pub last_limit_sync_at: Option<DateTimeUtc>,
    pub last_refresh_at: Option<DateTimeUtc>,
    pub last_error: Option<String>,
    pub failure_count: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
