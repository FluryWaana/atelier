use atelier_app::infrastructure::{
    brainstorm::{self, Convert},
    database::Database,
    runs,
    store::Documents,
};
use atelier_core::{application::brainstorm::Answer, contracts::Card, document::Document};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}

#[tokio::test]
async fn structured_command_conversation_converts_atomically_and_never_recreates_removed_tasks() {
    let temporary = tempfile::tempdir().unwrap();
    let repo = temporary.path().to_string_lossy().into_owned();
    let database = Database::open(&temporary.path().join("synthetic.sqlite")).unwrap();
    database.call(move|store|{
        let session:Card=serde_json::from_str(&json!({"id":"session","title":"Synthetic idea","description":"Keep idea","status":"todo","mode":"analyze","agent":"codex","repo":repo,"brainstormSession":true}).to_string())?;
        let project=document(json!({"id":"project","name":"Synthetic","repo":repo}));
        store.put(Documents::Cards,&session)?;store.put(Documents::Projects,&project)?;
        assert!(brainstorm::conversation(store,&session,&session,"Commence").is_ok());
        let run=document(json!({"id":"run","cardId":"session","repo":repo,"agentType":"command","purpose":"brainstorm","mode":"analyze","status":"starting","startedAt":"2026-10-25T10:00:00Z","userMessage":"Découpe en tâches"}));
        runs::reserve(store,&run)?;runs::transition(store,"run","running","synthetic-time",None)?;
        let output=json!({"message":"Voici le découpage","questions":[],"tickets":[{"title":"First","description":"First result","criteria":["Human validation"],"questions":["Keep this question"]},{"title":"Second","description":"Second result","criteria":["Observable result"],"questions":[]}]}).to_string();
        runs::append(store,"run","stdout",&output)?;runs::transition(store,"run","completed","synthetic-time",Some(0))?;
        let run=store.document::<Document>(Documents::Runs,"run")?.unwrap();assert!(run.get::<Answer>("brainstormAnswer")?.is_some());assert!(run.raw("structuredOutput").is_none());
        let prompt=brainstorm::conversation(store,&session,&session,"Précise la première tâche")?;assert!(prompt.contains("Découpe en tâches"));assert!(prompt.contains("Voici le découpage"));
        let mut input=Convert{expected:session.clone(),run_id:"run".into(),indices:vec![0,9],project:project.clone(),confirmed:true};
        assert!(brainstorm::convert(store,input.clone()).is_err());assert_eq!(store.documents::<Card>(Documents::Cards)?.len(),1);
        input.indices=vec![0,1];input.confirmed=false;assert!(brainstorm::convert(store,input.clone()).is_err());input.confirmed=true;
        let mut stale=input.clone();stale.expected.title="Stale".into();assert!(brainstorm::convert(store,stale).is_err());
        let created=brainstorm::convert(store,input.clone())?;assert_eq!(created.len(),2);
        assert!(created[0].fields.string_equals("status","todo")?);assert!(!created[0].fields.truthy("brainstormSession")?);
        assert!(created[0].fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap().iter().all(|criterion|!criterion.done));
        assert_eq!(created[0].fields.get::<Vec<String>>("brainstormQuestions")?.unwrap(),vec!["Keep this question"]);
        let mut changed=created[0].clone();changed.fields.set("description",&"Local changes")?;store.put(Documents::Cards,&changed)?;
        let repeated=brainstorm::convert(store,input.clone())?;assert_eq!(repeated.len(),2);assert!(repeated[0].fields.string_equals("description","Local changes")?);
        store.delete(Documents::Cards,&created[0].id)?;let repeated=brainstorm::convert(store,input)?;assert_eq!(repeated.len(),1);assert_eq!(store.documents::<Card>(Documents::Cards)?.len(),2);Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn malformed_or_overflowing_output_never_becomes_a_convertible_proposal() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        let card=document(json!({"id":"session","title":"Synthetic","brainstormSession":true}));store.put(Documents::Cards,&card)?;
        for (id,overflow) in [("invalid",false),("overflow",true)] {
            let run=document(json!({"id":id,"cardId":"session","repo":format!("synthetic-{id}"),"purpose":"brainstorm","agentType":"command","status":"starting"}));runs::reserve(store,&run)?;runs::transition(store,id,"running","time",None)?;
            if overflow {for _ in 0..5 {runs::append(store,id,"stdout",&"x".repeat(16*1024))?;}}else{runs::append(store,id,"stdout","{\"tickets\":[]}")?;}
            runs::transition(store,id,"completed","time",Some(0))?;let run=store.document::<Document>(Documents::Runs,id)?.unwrap();assert!(run.raw("brainstormAnswer").is_none());assert!(run.raw("brainstormError").is_some());assert!(run.raw("structuredOutput").is_none());
        }Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
