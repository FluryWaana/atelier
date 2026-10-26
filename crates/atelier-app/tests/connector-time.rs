use atelier_app::infrastructure::{
    connector_effects as effects, connectors, database::Database, store::Documents,
};
use atelier_core::{
    application::extensions::{
        self as protocol, Invocation, Method, RemotePreview, Reply, Report, Status,
    },
    contracts::ForgeWriteState as Phase,
    document::Document,
};
use serde_json::{Value, json};

fn doc(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}
const ENTRY: &str = "11111111-1111-4111-8111-111111111111";
fn card() -> Document {
    doc(json!({"id":"22222222-2222-4222-8222-222222222222","title":"Synthetic timed task"}))
}
fn entry() -> Document {
    doc(
        json!({"id":ENTRY,"cardId":"22222222-2222-4222-8222-222222222222","seconds":120,"date":"2026-10-26","startTime":"10:00:00","description":"Synthetic human work","exportState":"draft"}),
    )
}
fn settings() -> Document {
    doc(
        json!({"connectorRevision":1,"processConnectors":[{"manifest":{"id":"synthetic-time","name":"Synthetic time","description":"No network","sourceLocale":"en-US","methods":["check","timePreview","timeSend","timeReconcile"],"fields":[],"tabs":[]},"binary":{"path":"/synthetic/never-executed","sha256":"a".repeat(64)},"revision":1,"values":{},"report":null}]}),
    )
}
fn input(report: Option<&str>) -> Value {
    let mut input = json!({"expected":entry(),"expectedCard":card()});
    if let Some(report) = report {
        input["reportId"] = json!(report);
    }
    input
}

#[tokio::test]
async fn an_export_is_bound_to_the_entry_and_target_and_reconciles_without_resending() {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(|store| {
        store.save_settings(&settings())?;store.put(Documents::Cards,&card())?;store.put(Documents::Times,&entry())?;
        let invocation=Invocation{expected_revision:1,method:Method::TimePreview,action:None,input:input(None),confirmed:true};
        let (_,request)=connectors::invocation(store,"synthetic-time",&invocation,&mut|value,_|Ok(value.into()))?;
        assert_eq!(request.method,Method::TimePreview);
        assert!(request.operation_id.is_none());
        let proposal=json!({"source":request.input,"instance":"https://synthetic.example.test/time","account":"synthetic-account","reference":"SYN-1"});
        connectors::record(store,"synthetic-time",1,Report{id:"synthetic-report".into(),method:Method::TimePreview,action:None,message:"Synthetic preview".into(),output:proposal.clone(),observed_at:now()})?;
        let preview=effects::preview(store,"synthetic-time",&RemotePreview{expected_revision:1,method:Method::TimeSend,action:String::new(),input:input(Some("synthetic-report"))})?;
        let (_,send)=effects::prepare(store,"synthetic-time",&preview.id,preview.version,1,false,&mut|value,_|Ok(value.into()))?;
        assert_eq!(send.input,proposal);
        let sending=effects::sending(store,"synthetic-time",&preview.id,preview.version,1,&send.request_id)?;
        assert!(store.document::<Document>(Documents::Times,ENTRY)?.unwrap().string_equals("exportState","sending")?);
        let mut wrong=proposal.clone();wrong["source"]["seconds"]=json!(121);
        let receipt=json!({"operationId":sending.id,"outcome":"applied","reference":"synthetic-worklog","observedAt":now()});
        let reply=|request_id:String,method:Method,time:Value|Reply{protocol:protocol::PROTOCOL.into(),request_id,module:"synthetic-time".into(),method,status:Status::Ok,message:"Synthetic observation".into(),output:json!({"receipt":receipt,"time":time})};
        assert!(effects::record(store,"synthetic-time",&sending.id,sending.version,1,reply(send.request_id,Method::TimeSend,wrong),false).is_err());
        assert!(store.document::<Document>(Documents::Times,ENTRY)?.unwrap().string_equals("exportState","sending")?);
        effects::recover(store)?;
        let uncertain=effects::current(store,"synthetic-time",&sending.id)?;
        assert_eq!(uncertain.state,Phase::Uncertain);
        assert!(effects::prepare(store,"synthetic-time",&uncertain.id,uncertain.version,1,false,&mut|value,_|Ok(value.into())).is_err());
        let (_,read)=effects::prepare(store,"synthetic-time",&uncertain.id,uncertain.version,1,true,&mut|value,_|Ok(value.into()))?;
        assert_eq!(read.method,Method::TimeReconcile);
        assert_eq!(read.operation_id.as_deref(),Some(uncertain.id.as_str()));
        assert_eq!(read.input["method"],"timeSend");
        assert_eq!(read.input["input"],proposal);
        let observing=effects::observing(store,"synthetic-time",&uncertain.id,uncertain.version,1,&read.request_id)?;
        let reconciled=effects::record(store,"synthetic-time",&observing.id,observing.version,1,reply(read.request_id,Method::TimeReconcile,proposal),true)?;
        assert_eq!(reconciled.state,Phase::Reconciled);
        let entry=store.document::<Document>(Documents::Times,ENTRY)?.unwrap();
        assert!(entry.string_equals("exportState","exported")?);
        assert!(entry.string_equals("connectorExport",&reconciled.id)?);
        assert_eq!(entry.get::<u32>("seconds")?,Some(120));
        assert!(entry.string_equals("description","Synthetic human work")?);
        assert!(effects::preview(store,"synthetic-time",&RemotePreview{expected_revision:1,method:Method::TimeSend,action:String::new(),input:input(Some("synthetic-report"))}).is_err());
        Ok(())
    }).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn changed_entries_and_expired_or_foreign_proposals_are_refused_before_dispatch() {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(|store|{
        store.save_settings(&settings())?;store.put(Documents::Cards,&card())?;store.put(Documents::Times,&entry())?;
        let (_,request)=connectors::invocation(store,"synthetic-time",&Invocation{expected_revision:1,method:Method::TimePreview,action:None,input:input(None),confirmed:true},&mut|value,_|Ok(value.into()))?;
        let mut proposal=json!({"source":request.input,"instance":"https://synthetic.example.test/time","account":"synthetic-account","reference":"SYN-1"});
        proposal["source"]["seconds"]=json!(121);
        assert!(connectors::record(store,"synthetic-time",1,Report{id:"foreign".into(),method:Method::TimePreview,action:None,message:String::new(),output:proposal.clone(),observed_at:now()}).is_err());
        proposal["source"]["seconds"]=json!(120);
        connectors::record(store,"synthetic-time",1,Report{id:"expired".into(),method:Method::TimePreview,action:None,message:String::new(),output:proposal,observed_at:"2000-01-01T00:00:00Z".into()})?;
        assert!(effects::preview(store,"synthetic-time",&RemotePreview{expected_revision:1,method:Method::TimeSend,action:String::new(),input:input(Some("expired"))}).is_err());
        let mut changed=entry();changed.set("seconds",&121)?;store.put(Documents::Times,&changed)?;
        assert!(connectors::invocation(store,"synthetic-time",&Invocation{expected_revision:1,method:Method::TimePreview,action:None,input:input(None),confirmed:true},&mut|value,_|Ok(value.into())).is_err());
        assert!(effects::read(&store.settings::<Document>()?)?.is_empty());
        Ok(())
    }).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn only_an_explicit_not_attempted_receipt_releases_the_entry_for_a_new_confirmation() {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(|store|{
        store.save_settings(&settings())?;store.put(Documents::Cards,&card())?;store.put(Documents::Times,&entry())?;
        let (_,request)=connectors::invocation(store,"synthetic-time",&Invocation{expected_revision:1,method:Method::TimePreview,action:None,input:input(None),confirmed:true},&mut|value,_|Ok(value.into()))?;
        connectors::record(store,"synthetic-time",1,Report{id:"ready".into(),method:Method::TimePreview,action:None,message:String::new(),output:json!({"source":request.input,"instance":"https://synthetic.example.test/time","account":"synthetic-account","reference":"SYN-1"}),observed_at:now()})?;
        let prepare=RemotePreview{expected_revision:1,method:Method::TimeSend,action:String::new(),input:input(Some("ready"))};
        let operation=effects::preview(store,"synthetic-time",&prepare)?;
        let sending=effects::sending(store,"synthetic-time",&operation.id,operation.version,1,"request-synthetic-send")?;
        let failed=effects::record(store,"synthetic-time",&sending.id,sending.version,1,Reply{protocol:protocol::PROTOCOL.into(),request_id:"request-synthetic-send".into(),module:"synthetic-time".into(),method:Method::TimeSend,status:Status::Error,message:"Synthetic preflight refused before any write".into(),output:json!({"receipt":{"operationId":sending.id,"outcome":"notAttempted","reference":"synthetic-preflight","observedAt":now()}})},false)?;
        assert_eq!(failed.state,Phase::Failed);
        let entry=store.document::<Document>(Documents::Times,ENTRY)?.unwrap();
        assert!(entry.string_equals("exportState","draft")?);
        assert!(entry.raw("connectorExport").is_none());
        assert_eq!(effects::read(&store.settings::<Document>()?)?.len(),1);
        let next=effects::preview(store,"synthetic-time",&prepare)?;
        assert_ne!(next.id,failed.id);
        assert_eq!(effects::read(&store.settings::<Document>()?)?.len(),2);
        assert_eq!(effects::current(store,"synthetic-time",&failed.id)?.state,Phase::Failed);
        Ok(())
    }).await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn a_human_edit_after_preparation_is_preserved_and_refuses_the_send_reservation() {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(|store|{
        store.save_settings(&settings())?;store.put(Documents::Cards,&card())?;store.put(Documents::Times,&entry())?;
        let (_,request)=connectors::invocation(store,"synthetic-time",&Invocation{expected_revision:1,method:Method::TimePreview,action:None,input:input(None),confirmed:true},&mut|value,_|Ok(value.into()))?;
        connectors::record(store,"synthetic-time",1,Report{id:"ready".into(),method:Method::TimePreview,action:None,message:String::new(),output:json!({"source":request.input,"instance":"https://synthetic.example.test/time","account":"synthetic-account","reference":"SYN-1"}),observed_at:now()})?;
        let operation=effects::preview(store,"synthetic-time",&RemotePreview{expected_revision:1,method:Method::TimeSend,action:String::new(),input:input(Some("ready"))})?;
        let mut changed=entry();changed.set("seconds",&121)?;store.put(Documents::Times,&changed)?;
        assert!(effects::sending(store,"synthetic-time",&operation.id,operation.version,1,"request-synthetic-send").is_err());
        assert_eq!(effects::current(store,"synthetic-time",&operation.id)?.state,Phase::Preparing);
        let cancelled=effects::cancel(store,"synthetic-time",&operation.id,operation.version)?;
        assert_eq!(cancelled.state,Phase::Failed);
        let saved=store.document::<Document>(Documents::Times,ENTRY)?.unwrap();
        assert_eq!(saved.get::<u32>("seconds")?,Some(121));
        assert!(saved.raw("connectorExport").is_none());
        assert!(saved.string_equals("exportState","draft")?);
        Ok(())
    }).await.unwrap();
    db.close().await.unwrap();
}
