use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "request_records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub credential_id: String,
    pub credential_name: String,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub principal_kind: String,
    pub transport: String,
    pub request_method: String,
    pub request_path: String,
    pub upstream_status_code: Option<i32>,
    pub request_success: Option<bool>,
    pub error_phase: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub response_id: Option<String>,
    pub requested_model: Option<String>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub usage_json: Option<String>,
    pub request_started_at: DateTimeUtc,
    pub request_completed_at: Option<DateTimeUtc>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
