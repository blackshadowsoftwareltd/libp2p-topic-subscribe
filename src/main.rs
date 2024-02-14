use async_std::io;
use futures::{AsyncBufReadExt, StreamExt};
use libp2p::{
    gossipsub::{self, IdentTopic},
    mdns, noise,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, StreamProtocol, Swarm,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};
use tokio::select;
use tracing_subscriber::EnvFilter;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

#[tokio::main]
async fn main() {
    init_topics().await;
    let mut swarm = config();

    let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    loop {
        select! {
            Ok(line)  = stdin.select_next_some() => {
                let line:String = line.to_owned();
                println!("Matched: {:?}", line.contains("subscribe"));
                if line.contains("subscribe"){

                    subscribe(&mut swarm, line.clone());
                }else{
                    for t in get_topics().iter(){
                      match  swarm
                        .behaviour_mut().gossipsub
                        .publish(t.clone(), line.as_bytes()){
                            Ok(_)=>println!("Published to topic: {:?}", t),
                            Err(e)=>println!("Error: {:?}", e)
                        }
                    }

                }
            }
            event = swarm.select_next_some() => {
                let behaviour = swarm.behaviour_mut();
                match event {
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer, addr) in list {
                        println!("Discovered peer: {} with address {}", peer, addr);
                        behaviour.gossipsub.add_explicit_peer(&peer);


                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _multiaddr) in list {
                        println!("mDNS discover peer has expired: {peer_id}");
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    }
                },
                SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                })) => println!(
                        "Got message: '{}' with id: {id} from peer: {peer_id}",
                        String::from_utf8_lossy(&message.data),
                    ),
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Local node is listening on {address}");
                }
                SwarmEvent::Behaviour(MyBehaviourEvent::RequestResponse(e)) => match e {
                    libp2p::request_response::Event::Message { message, .. } => match message {
                        libp2p::request_response::Message::Request {
                            request, channel, ..
                        } => {
                            println!("Request ---> {:?}", request.data);
                            let r = ReqResModel {
                                data: "Messege Received".to_string(),
                            };
                       let x= swarm.behaviour_mut().request_response.send_response(channel, r);
                            println!("---> Sent Response: {:?}", x);
                        }
                        libp2p::request_response::Message::Response { response, .. } => {
                            println!("Response ---> {:?}", response.data);
                        }
                    },
                    _ => {
                    println!("Unhandled event: gc ");

                    }
                }
                SwarmEvent::ConnectionEstablished{peer_id,..}=>{
                    let v= ReqResModel{data:"hello".to_string()};
                    let id= behaviour.request_response.send_request(&peer_id,v);
                    println!("---> Sent ID: {:?}", id);
                }
                        _ => {
                    // println!("Unhandled event: {event:?}");
                }
            }}
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqResModel {
    data: String,
}

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    request_response: request_response::cbor::Behaviour<ReqResModel, ReqResModel>,
    gossipsub: gossipsub::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

pub fn config() -> Swarm<MyBehaviour> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_behaviour(|key| {
            // To content-address message, we can take the hash of message and use it as an ID.
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };

            // Set a custom gossipsub configuration
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(3)) // This is set to aid debugging by not cluttering the log space
                .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
                .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?; // Temporary hack because `build` does not return a proper `std::error::Error`.

            // build a gossipsub network behaviour
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let request_response = request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/my-cbor-protocol"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            );
            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;
            Ok(MyBehaviour {
                request_response,
                gossipsub,
                mdns,
            })
        })
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Create a Gossipsub topic
    // let topic = gossipsub::IdentTopic::new("2");
    // subscribes to our topic
    // swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();
    swarm
}

fn subscribe(swarm: &mut Swarm<MyBehaviour>, s: String) {
    let topic = s;
    topic.replace("subscribe", "").trim().to_string();
    println!("Subscribing to topic: {topic}");
    add_topic(topic.to_string());
    let topic = gossipsub::IdentTopic::new(topic.as_str());
    swarm.behaviour_mut().gossipsub.subscribe(&topic).unwrap();
}

pub static TOPICS: OnceLock<Mutex<HashMap<String, IdentTopic>>> = OnceLock::new();

pub async fn init_topics() {
    let mut _peers = TOPICS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
}

pub fn add_topic(t: String) {
    let topic = gossipsub::IdentTopic::new(t.clone());
    TOPICS.get().unwrap().lock().unwrap().insert(t, topic);
}

pub fn remove_topic(t: String) {
    TOPICS.get().unwrap().lock().unwrap().remove_entry(&t);
}

pub fn get_topics() -> Vec<IdentTopic> {
    let mut topics: Vec<IdentTopic> = vec![];
    for (_, v) in TOPICS.get().unwrap().lock().unwrap().iter() {
        topics.push(v.clone());
    }

    topics
}
