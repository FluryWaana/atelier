use atelier_app::infrastructure::{database::Database, preparation, runs, store::Documents};
use atelier_core::{
    application::preparation::{self as domain, Apply},
    contracts::Card,
    document::Document,
};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
#[tokio::test]
async fn preparation_is_snapshot_bound_explicit_and_cannot_reset_validation_on_replay() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        let original:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic task","description":"Keep description","instructions":"Keep instructions","status":"todo","checklist":[{"text":"Old criterion","done":true}]}).to_string())?;store.put(Documents::Cards,&original)?;
        let mut run=document(json!({"id":"preparation","cardId":"task","repo":"synthetic","purpose":"preparation","agentType":"command","status":"starting","requirements":domain::requirements(&original)?}));
        runs::reserve(store,&run)?;runs::transition(store,"preparation","running","time",None)?;
        runs::append(store,"preparation","stdout",&json!({"criteria":["New observable criterion"],"questions":["Open question"]}).to_string())?;
        runs::transition(store,"preparation","completed","time",Some(0))?;
        run=store.document::<Document>(Documents::Runs,"preparation")?.unwrap();assert!(run.get::<domain::Preparation>("preparation")?.is_some());
        let input=||Apply{expected:original.clone(),run_id:"preparation".into(),confirmed:true};
        let mut unconfirmed=input();unconfirmed.confirmed=false;assert!(preparation::apply(store,unconfirmed).is_err());
        let mut changed=original.clone();changed.fields.set("description",&"Changed need")?;store.put(Documents::Cards,&changed)?;
        assert!(preparation::apply(store,input()).is_err());
        assert!(preparation::apply(store,Apply{expected:changed,run_id:"preparation".into(),confirmed:true}).is_err());store.put(Documents::Cards,&original)?;
        store.put(Documents::Runs,&document(json!({"id":"busy","cardId":"task","status":"running"})))?;assert!(preparation::apply(store,input()).is_err());store.delete(Documents::Runs,"busy")?;
        let mut applied=preparation::apply(store,input())?;assert!(applied.fields.string_equals("status","todo")?);assert!(applied.fields.string_equals("description","Keep description")?);assert!(applied.fields.string_equals("instructions","Keep instructions")?);
        let mut checklist=applied.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap();assert_eq!(checklist.len(),1);assert!(!checklist[0].done);checklist[0].done=true;applied.fields.set("checklist",&checklist)?;store.put(Documents::Cards,&applied)?;
        assert!(preparation::apply(store,Apply{expected:applied.clone(),run_id:"preparation".into(),confirmed:true}).is_err());let retained=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert!(retained.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap()[0].done);
        assert_eq!(retained.fields.get::<Vec<String>>("preparationQuestions")?.unwrap(),vec!["Open question"]);Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
