use crate::data::attr::{Attribute, AttributeCardinality, AttributeIndex, AttributeTyping};
use crate::data::encode::EncodedVec;
use crate::data::id::{AttrId, EntityId, Validity};
use crate::data::keyword::Keyword;
use crate::data::value::Value;
use crate::Db;
use anyhow::Result;
use cozorocks::DbBuilder;

fn create_db(name: &str) -> Db {
    let builder = DbBuilder::default()
        .path(name)
        .create_if_missing(true)
        .destroy_on_exit(true);
    Db::build(builder).unwrap()
}

fn test_send_sync<T: Send + Sync>(_: &T) {}

#[test]
fn creation() {
    let db = create_db("_test_db");
    test_send_sync(&db);
    let current_validity = Validity::current();
    let session = db.new_session().unwrap();
    let mut tx = session.transact().unwrap();
    assert_eq!(
        0,
        tx.all_attrs()
            .collect::<Result<Vec<Attribute>>>()
            .unwrap()
            .len()
    );

    let mut tx = session.transact_write().unwrap();
    tx.new_attr(Attribute {
        id: AttrId(0),
        keyword: Keyword::try_from("hello/world").unwrap(),
        cardinality: AttributeCardinality::Many,
        val_type: AttributeTyping::Int,
        indexing: AttributeIndex::None,
        with_history: true,
    })
    .unwrap();
    tx.commit_tx("", false).unwrap();

    let mut tx = session.transact_write().unwrap();
    let attr = tx
        .attr_by_kw(&Keyword::try_from("hello/world").unwrap())
        .unwrap()
        .unwrap();
    tx.new_triple(EntityId(1), &attr, &Value::Int(98765), current_validity)
        .unwrap();
    tx.new_triple(EntityId(2), &attr, &Value::Int(1111111), current_validity)
        .unwrap();
    tx.commit_tx("haah", false).unwrap();

    let mut tx = session.transact_write().unwrap();
    tx.amend_attr(Attribute {
        id: AttrId(10000001),
        keyword: Keyword::try_from("hello/sucker").unwrap(),
        cardinality: AttributeCardinality::Many,
        val_type: AttributeTyping::Int,
        indexing: AttributeIndex::None,
        with_history: true,
    })
    .unwrap();
    tx.commit_tx("oops", false).unwrap();

    let mut tx = session.transact().unwrap();
    let world_found = tx
        .attr_by_kw(&Keyword::try_from("hello/world").unwrap())
        .unwrap();
    dbg!(world_found);
    let sucker_found = tx
        .attr_by_kw(&Keyword::try_from("hello/sucker").unwrap())
        .unwrap();
    dbg!(sucker_found);
    for attr in tx.all_attrs() {
        dbg!(attr.unwrap());
    }

    for r in tx.triple_a_scan_all() {
        dbg!(r.unwrap());
    }

    dbg!(&session);

    let mut it = session.total_iter();
    while let Some((k, v)) = it.pair().unwrap() {
        let key = EncodedVec::new(k);
        let val = key.debug_value(v);
        dbg!(key);
        dbg!(val);
        it.next();
    }
}