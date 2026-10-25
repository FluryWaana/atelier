use atelier_app::infrastructure::{
    backups::BackupDirectory, database::Database, store::Documents, tempo_export as workflow,
};
use atelier_core::{
    application::time_tracking,
    contracts::{
        Card, ForgeWriteState as Phase, TempoConfiguration, TempoExportIntent, TempoExportRequest,
        TempoPayload,
    },
    document::Document,
};
use serde_json::json;

#[tokio::test]
async fn exact_preview_reservation_restart_and_observation_preserve_human_time_and_backup_history()
{
    let home = tempfile::tempdir().unwrap();
    let path = home.path().to_owned();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(move|store| {
        store.save_settings(&json!({"jiraConnection":{"mode":"cloud","url":"https://synthetic-jira.example.test","email":"synthetic@example.invalid","query":"project = SYN","revision":1},"tempoConnection":{"base":"https://api.tempo.io/4","revision":1}}))?;
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Keep human title","description":"Keep human description","ticket":{"connector":"jira","instance":"https://synthetic-jira.example.test","id":"123","key":"SYN-1"},"checklist":[{"text":"Human criterion","done":true}]}).to_string())?;
        store.put(Documents::Cards,&card)?;
        let entry_id=uuid::Uuid::new_v4().to_string();
        let original:Document=serde_json::from_str(&json!({"id":entry_id,"cardId":"task","seconds":3600,"date":"2026-10-25","startTime":"09:30:00","description":"Synthetic human time","exportState":"draft"}).to_string())?;
        store.put(Documents::Times,&original)?;
        let backups=BackupDirectory::open(&path)?;let old=backups.create(store)?;
        let configuration=TempoConfiguration{base:"https://api.tempo.io/4".into(),revision:1};
        let mut input=TempoExportRequest{expected:original.clone(),expected_card:card.clone(),jira_revision:1,tempo_revision:1,confirmed:false};
        let payload=TempoPayload{issue_id:123,author_account_id:"synthetic-account".into(),time_spent_seconds:3600,start_date:"2026-10-25".into(),start_time:"09:30:00".into(),description:format!("Synthetic human time\n[atelier:{entry_id}]")};
        let mut wrong=payload.clone();wrong.time_spent_seconds=60;
        assert!(workflow::save_preview(store,&input,configuration.clone(),wrong).is_err());
        input.expected=workflow::save_preview(store,&input,configuration.clone(),payload.clone())?;
        let operation_id=uuid::Uuid::new_v4().to_string();
        assert!(workflow::reserve(store,&input,&operation_id).is_err());input.confirmed=true;
        let mut changed=input.expected.clone();changed.set("seconds",&60)?;store.put(Documents::Times,&changed)?;
        assert!(workflow::reserve(store,&input,&operation_id).is_err());store.put(Documents::Times,&input.expected)?;
        let sending=workflow::reserve(store,&input,&operation_id)?;
        assert!(sending.string_equals("exportState","sending")?);
        assert!(workflow::reserve(store,&input,&operation_id).is_err());
        workflow::remember(store,&entry_id,&operation_id,7)?;
        time_tracking::initialize(store)?;
        let recovered=store.document::<Document>(Documents::Times,&entry_id)?.unwrap();
        let intent=recovered.get::<TempoExportIntent>("tempoExport")?.unwrap();
        assert_eq!(intent.state,Phase::Uncertain);assert_eq!(intent.worklog_id,Some(7));assert_eq!(intent.preview.payload,payload);
        for path in ["/api/cards/task/worktree/remove","/api/cards/task/runs/start","/api/projects","/api/backups"] {assert!(workflow::blocks(store,path)?);}
        assert!(!workflow::blocks(store,&format!("/api/time/{entry_id}/reconcile"))?);
        assert!(backups.restore(store,&old.name).is_err());
        assert!(workflow::record(store,&entry_id,&operation_id,8,1,1).is_err());
        assert!(workflow::record(store,&entry_id,&operation_id,7,0,1).is_err());
        assert!(workflow::record(store,&entry_id,&operation_id,7,1,0).is_err());
        let exported=workflow::record(store,&entry_id,&operation_id,7,1,1)?;
        assert!(exported.string_equals("exportState","exported")?);assert!(exported.string_equals("description","Synthetic human time")?);
        assert_eq!(exported.get::<u32>("seconds")?,Some(3600));assert!(!workflow::blocks(store,"/api/cards/task")?);
        assert!(workflow::record(store,&entry_id,&operation_id,7,1,1).is_err());assert!(backups.restore(store,&old.name).is_err());
        let unchanged=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(serde_json::to_value(unchanged)?,serde_json::to_value(&card)?);
        let before_manual_reset=backups.create(store)?;
        // A second synthetic entry exercises the human override independently of the observed write.
        let second_id=uuid::Uuid::new_v4().to_string();let mut second=original;second.set("id",&second_id)?;store.put(Documents::Times,&second)?;
        input.expected=second;input.confirmed=false;let mut second_payload=payload;second_payload.description=format!("Synthetic human time\n[atelier:{second_id}]");
        input.expected=workflow::save_preview(store,&input,configuration,second_payload)?;input.confirmed=true;
        let second_operation=uuid::Uuid::new_v4().to_string();workflow::reserve(store,&input,&second_operation)?;time_tracking::initialize(store)?;
        let uncertain=store.document::<Document>(Documents::Times,&second_id)?.unwrap();
        let mut reset:Document=serde_json::from_str(&json!({"expected":uncertain,"confirmed":true,"manualEvidence":"Synthetic manual decision, no remote proof asserted"}).to_string())?;
        assert!(workflow::reset(store,&second_id,&reset).is_err());reset.set("acceptDuplicateRisk",&true)?;
        let reset=workflow::reset(store,&second_id,&reset)?;
        assert!(reset.string_equals("exportState","draft")?);assert!(reset.raw("tempoExport").is_none());
        let history=reset.get::<Vec<serde_json::Value>>("tempoExportHistory")?.unwrap();assert_eq!(history.len(),1);assert_eq!(history[0]["intent"]["preview"]["payload"]["description"],format!("Synthetic human time\n[atelier:{second_id}]"));
        assert!(workflow::record(store,&second_id,&second_operation,99,1,1).is_err());
        assert!(backups.restore(store,&before_manual_reset.name).is_err());
        assert!(backups.restore(store,&old.name).is_err());
        assert!(time_tracking::execute(store,&atelier_app::infrastructure::time_tracking::Clock,"remove",&second_id,&serde_json::from_str(&json!({"expected":reset}).to_string())?,||"unused".into(),|_|false).is_err());
        assert!(store.document::<Document>(Documents::Times,&second_id)?.is_some());
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
