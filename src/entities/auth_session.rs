use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "auth_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub credential_id: String,
    pub method: String,
    pub status: String,
    pub authorization_url: Option<String>,
    pub redirect_uri: Option<String>,
    pub oauth_state: Option<String>,
    pub pkce_code_verifier: Option<String>,
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
    pub device_auth_id: Option<String>,
    pub device_code_interval_seconds: Option<i32>,
    pub error_message: Option<String>,
    pub completed_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
