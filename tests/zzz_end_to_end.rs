#![cfg(not(feature = "skip-end-to-end"))]
use env_logger;
use tokio;

#[macro_use]
extern crate log;

use std::time::Duration;
use stomping::*;
use uuid::Uuid;

#[tokio::test]
async fn can_round_trip_text() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(async {
        debug!("Starting connection");
        let res = conn.await;
        debug!("Connection terminated: {:?}", res);
    });

    let body = b"42";
    let queue = format!("/queue/can_round_trip_text-{}", Uuid::new_v4());

    info!("Subscribing to queue");
    client
        .subscribe(&queue, "one", AckMode::Auto)
        .await
        .expect("subscribe");

    info!("Publishing to queue");
    client.publish(&queue, body).await.expect("publish");

    info!("Consuming from queue");
    let frame = client.consume_next().await.expect("consume_next");
    info!("Consumed item");

    assert_eq!(&*body, &*frame.body);
    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn can_round_trip_binary_blobs() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    let body = b"\x00\x01\x02\x03";
    let queue = format!("/queue/can_round_trip_binary_blobs-{}", Uuid::new_v4());

    client
        .subscribe(&queue, "one", AckMode::Auto)
        .await
        .expect("subscribe");
    client.publish(&queue, body).await.expect("publish");

    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, body);
    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn client_acks_should_allow_redelivery() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    let body = b"42";
    let queue = format!(
        "/queue/client_acks_should_allow_redelivery-{}",
        Uuid::new_v4()
    );

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");
    client.publish(&queue, body).await.expect("publish");

    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, body);

    // Disconnect
    drop(client);

    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");
    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, body);
    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn can_encode_headers_correctly() {
    env_logger::try_init().unwrap_or_default();
    debug!("Connecting");
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    let body = b"42";
    let queue = format!("/queue/can_encode_headers_correctly:{}", Uuid::new_v4());

    client
        .subscribe(&queue, "one", AckMode::Auto)
        .await
        .expect("subscribe");
    client.publish(&queue, body).await.expect("publish");

    let frame = client.consume_next().await.expect("consume_next");
    println!("h: {:?}", frame.headers);
    assert_eq!(
        std::str::from_utf8(&frame.headers["destination".as_bytes()]).expect("from utf8"),
        queue
    );
    client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn should_allow_acking_individual_messages() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    let conn = tokio::spawn(conn);

    let queue = format!(
        "/queue/client_acks_should_allow_redelivery-{}",
        Uuid::new_v4()
    );

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");
    client.publish(&queue, b"first").await.expect("publish");
    client.publish(&queue, b"second").await.expect("publish");
    client.publish(&queue, b"third").await.expect("publish");

    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, b"first");

    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, b"second");
    debug!(
        "Acking second: {:?}",
        frame
            .headers
            .get("ack".as_bytes())
            .map(|h| String::from_utf8_lossy(h))
    );
    client.ack(&frame.headers).await.expect("ack");

    // Disconnect
    drop(client);
    // wait for the connection to exit
    conn.await.expect("join").expect("connection exit");

    debug!("Reconnecting");
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");
    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(String::from_utf8_lossy(&frame.body), "first");
    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(String::from_utf8_lossy(&frame.body), "third");
    client.disconnect().await.expect("disconnect");
}

// This should be replaced with useful use of timeouts.
#[cfg(todo)]
#[tokio::test]
async fn should_allow_timeout_on_consume() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(("localhost", 61613), Some(("guest", "guest")), None)
        .await
        .expect("connect");
    tokio::spawn(conn);

    let queue = format!(
        "/queue/client_acks_should_allow_redelivery-{}",
        Uuid::new_v4()
    );

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");
    let timeout = Duration::from_millis(500);
    let cons_start = SystemTime::now();
    debug!("Starting consume at {:?}", cons_start);
    // was maybe_consume_next(timeout)
    let resp = client.consume_next().await.expect("consume_next");
    let duration = cons_start.elapsed().expect("elapsed");
    debug!("consume done in {:?}", duration);
    assert!(resp.is_none());
    assert!(duration >= timeout);

    client.publish(&queue, b"first").await.expect("publish");
    // was maybe_consume_next(timeout)
    let resp = client.consume_next().await.expect("consume_next");
    let frame = resp.expect("a message");
    assert_eq!(frame.body, b"first");
}

// This test never actually terminates.
#[tokio::test]
#[ignore]
async fn thing_to_test_timeouts() {
    env_logger::try_init().unwrap_or_default();
    let (conn, mut client) = connect(
        ("localhost", 61613),
        Some(("guest", "guest")),
        Some(Duration::from_millis(500)),
    )
    .await
    .expect("connect");
    tokio::spawn(conn);

    let queue = format!("/queue/thing_to_test_timeouts-{}", Uuid::new_v4());

    client
        .subscribe(&queue, "one", AckMode::ClientIndividual)
        .await
        .expect("subscribe");

    let frame = client.consume_next().await.expect("consume_next");
    assert_eq!(frame.body, b"first");
    client.disconnect().await.expect("disconnect");
}
