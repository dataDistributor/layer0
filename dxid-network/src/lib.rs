use anyhow::Result;
use async_trait::async_trait;
use dxid_core::{Block, Transaction};
use futures::{channel::mpsc, prelude::*};
use libp2p::gossipsub::{
    self, IdentTopic as Topic, MessageAuthenticity, MessageId, ValidationMode,
};
use libp2p::identity::Keypair;
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmBuilder, SwarmEvent};
use libp2p::{identify, mdns, noise, tcp, yamux, Multiaddr, PeerId, Transport};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info};
use crate::DxidBehaviourEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: String,
    pub seed_nodes: Vec<String>,
}

#[async_trait]
pub trait NetworkService: Send + Sync {
    async fn start(&mut self) -> Result<()>;
    async fn broadcast_block(&mut self, block: Block) -> Result<()>;
    async fn broadcast_tx(&mut self, tx: Transaction) -> Result<()>;
    fn local_peer_id(&self) -> PeerId;
}

#[derive(NetworkBehaviour)]
struct DxidBehaviour {
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

pub struct Libp2pNetwork {
    swarm: Swarm<DxidBehaviour>,
    block_topic: Topic,
    tx_topic: Topic,
    peers: HashSet<PeerId>,
    handle: Option<JoinHandle<()>>,
}

impl Libp2pNetwork {
    pub fn new(config: NetworkConfig) -> Result<Self> {
        let local_key = Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();

        let message_id_fn = |m: &gossipsub::Message| {
            MessageId::from(blake3::hash(&m.data).to_hex().to_string())
        };

        let mut gossipsub_config = gossipsub::ConfigBuilder::default()
            .message_id_fn(message_id_fn)
            .validation_mode(ValidationMode::Strict)
            .build()
            .expect("gossipsub config");

        let gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        let identify = identify::Behaviour::new(identify::Config::new(
            "/dxid/0.1".into(),
            local_key.public(),
        ));

        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        let behaviour = DxidBehaviour {
            gossipsub,
            identify,
            mdns,
        };

        let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();

        let listen_addr: Multiaddr = config.listen_addr.parse()?;
        swarm.listen_on(listen_addr)?;

        for addr in config.seed_nodes {
            if let Ok(ma) = addr.parse() {
                swarm.dial(ma)?;
            }
        }

        Ok(Self {
            swarm,
            block_topic: Topic::new("dxid-blocks"),
            tx_topic: Topic::new("dxid-transactions"),
            peers: HashSet::new(),
            handle: None,
        })
    }
}

#[async_trait]
impl NetworkService for Libp2pNetwork {
    async fn start(&mut self) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.block_topic)?;
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.tx_topic)?;
        let mut swarm = std::mem::replace(&mut self.swarm, build_empty_swarm()?);
        let block_topic = self.block_topic.clone();
        let tx_topic = self.tx_topic.clone();
        self.handle = Some(tokio::spawn(async move {
            loop {
                match swarm.select_next_some().await {
                    SwarmEvent::Behaviour(DxidBehaviourEvent::Gossipsub(ev)) => match ev {
                        gossipsub::Event::Message {
                            propagation_source,
                            message_id,
                            message,
                        } => {
                            debug!("gossip from {propagation_source:?} id {message_id:?} len {}", message.data.len());
                        }
                        gossipsub::Event::Subscribed { peer_id, .. } => {
                            debug!("peer subscribed {peer_id}");
                        }
                        _ => {}
                    },
                    SwarmEvent::Behaviour(DxidBehaviourEvent::Mdns(ev)) => match ev {
                        mdns::Event::Discovered(list) => {
                            for (peer, addr) in list {
                                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                                debug!("mdns discovered {peer} at {addr}");
                            }
                        }
                        mdns::Event::Expired(_) => {}
                    },
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("listening on {address}");
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        info!("peer connected {peer_id}");
                    }
                    _ => {}
                }
            }
        }));
        Ok(())
    }

    async fn broadcast_block(&mut self, block: Block) -> Result<()> {
        let data = serde_json::to_vec(&block)?;
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.block_topic.clone(), data)?;
        Ok(())
    }

    async fn broadcast_tx(&mut self, tx: Transaction) -> Result<()> {
        let data = serde_json::to_vec(&tx)?;
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.tx_topic.clone(), data)?;
        Ok(())
    }

    fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }
}

fn build_empty_swarm() -> Result<Swarm<DxidBehaviour>> {
    let local_key = Keypair::generate_ed25519();
    let transport = tcp::tokio::Transport::new(tcp::Config::default())
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .boxed();
    let gossipsub = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(local_key.clone()),
        gossipsub::Config::default(),
    )?;
    let identify = identify::Behaviour::new(identify::Config::new(
        "/dxid/0.1".into(),
        local_key.public(),
    ));
    let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), PeerId::from(local_key.public()))?;
    let behaviour = DxidBehaviour {
        gossipsub,
        identify,
        mdns,
    };
    Ok(SwarmBuilder::with_tokio_executor(
        transport,
        behaviour,
        PeerId::from(local_key.public()),
    )
    .build())
}
