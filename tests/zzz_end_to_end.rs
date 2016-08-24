extern crate stomping;
extern crate env_logger;
extern crate log;
extern crate uuid;

use stomping::*;
use uuid::Uuid;

#[test]
fn can_round_trip_text() {
    env_logger::init().unwrap_or(());
    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    let body = b"42";
    let queue = format!("/queue/can_round_trip_text-{}", Uuid::new_v4());

    client.subscribe(&queue, "one", AckMode::Auto).expect("subscribe");
    client.publish(&queue, body).expect("publish");

    let (_headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, body);
}

#[test]
fn can_round_trip_binary_blobs() {
    env_logger::init().unwrap_or(());
    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    let body = b"\x00\x01\x02\x03";
    let queue = format!("/queue/can_round_trip_binary_blobs-{}", Uuid::new_v4());

    client.subscribe(&queue, "one", AckMode::Auto).expect("subscribe");
    client.publish(&queue, body).expect("publish");

    let (headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, body);
}

#[test]
fn client_acks_should_allow_redelivery() {
    env_logger::init().unwrap_or(());
    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    let body = b"42";
    let queue = format!("/queue/client_acks_should_allow_redelivery-{}", Uuid::new_v4());

    client.subscribe(&queue, "one", AckMode::ClientIndividual).expect("subscribe");
    client.publish(&queue, body).expect("publish");

    let (_headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, body);

    // Disconnect
    drop(client);

    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    client.subscribe(&queue, "one", AckMode::ClientIndividual).expect("subscribe");
    let (_headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, body);
}

#[test]
fn should_allow_acking_individual_messages() {
    env_logger::init().unwrap_or(());
    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    let queue = format!("/queue/client_acks_should_allow_redelivery-{}", Uuid::new_v4());

    client.subscribe(&queue, "one", AckMode::ClientIndividual).expect("subscribe");
    client.publish(&queue, b"first").expect("publish");
    client.publish(&queue, b"second").expect("publish");

    let (_headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, b"first");
    let (headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, b"second");
    client.ack(&headers);

    // Disconnect
    drop(client);

    let mut client = Client::connect(("localhost", 61613), Some(("guest", "guest"))).expect("connect");
    client.subscribe(&queue, "one", AckMode::ClientIndividual).expect("subscribe");
    let (_headers, msg) = client.consume_next().expect("consume_next");
    assert_eq!(msg, b"first");
}
