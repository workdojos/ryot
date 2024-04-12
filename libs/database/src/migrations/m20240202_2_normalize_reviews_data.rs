use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_column("review", "extra_information").await? {
            let db = manager.get_connection();
            db.execute_unprepared(
                r#"
ALTER TABLE review ADD COLUMN show_extra_information JSONB;
ALTER TABLE review ADD COLUMN podcast_extra_information JSONB;
ALTER TABLE review ADD COLUMN studies_extra_information JSONB;
ALTER TABLE review ADD COLUMN comic_extra_information JSONB;

UPDATE review SET show_extra_information = extra_information -> 'Show' WHERE extra_information -> 'Show' IS NOT NULL;
UPDATE review SET podcast_extra_information = extra_information -> 'Podcast' WHERE extra_information -> 'Podcast' IS NOT NULL;
UPDATE review SET studies_extra_information = extra_information -> 'Studies' WHERE extra_information -> 'Studies' IS NOT NULL;
UPDATE review SET comic_extra_information = extra_information -> 'Comic' WHERE extra_information -> 'Comic' IS NOT NULL;

ALTER TABLE review DROP COLUMN extra_information;
"#,
)
.await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
