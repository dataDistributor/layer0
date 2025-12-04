use anyhow::Result;
use async_trait::async_trait;
use blake3::Hasher;
use dxid_core::{ChainMetadata, CrossChainMessage, CryptoProvider, BlockHeader, Address, BlockHash};
use ed25519_dalek::{Signature, Signer, Verifier, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ark_bls12_381::Bls12_381;
use ark_ff::PrimeField;
use ark_groth16::{prepare_verifying_key, Groth16, Proof, ProvingKey, r1cs_to_qap::LibsnarkReduction};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::rand::thread_rng;
use winterfell::crypto::DefaultRandomCoin;
use winterfell::math::{fields::f64::BaseElement, FieldElement, ToElements};
use winterfell::{ProofOptions, StarkProof, Prover, Trace};
use std::convert::TryInto;

#[derive(Debug, Clone)]
pub struct KeyMaterial {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

pub fn generate_ed25519() -> KeyMaterial {
    let mut csprng = OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verify = signing.verifying_key();
    KeyMaterial {
        public_key: verify.to_bytes().to_vec(),
        secret_key: signing.to_bytes().to_vec(),
    }
}

pub struct DefaultCryptoProvider;

impl DefaultCryptoProvider {
    pub fn new() -> Self {
        Self
    }
}

impl CryptoProvider for DefaultCryptoProvider {
    fn address_from_public_key(&self, pk: &[u8]) -> Result<Address> {
        let mut hasher = Hasher::new();
        hasher.update(pk);
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(digest.as_bytes());
        Ok(out)
    }

    fn verify_signature(&self, pk: &[u8], msg: &[u8], sig: &[u8]) -> Result<bool> {
        let pk_arr: [u8; 32] = pk.try_into().map_err(|_| anyhow::anyhow!("bad pk length"))?;
        let vk = VerifyingKey::from_bytes(&pk_arr)?;
        let sig_arr: [u8; SIGNATURE_LENGTH] =
            sig.try_into().map_err(|_| anyhow::anyhow!("bad sig length"))?;
        let signature = Signature::from_bytes(&sig_arr);
        Ok(vk.verify(msg, &signature).is_ok())
    }

    fn sign_message(&self, sk: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
        let sk_arr: [u8; 32] = sk.try_into().map_err(|_| anyhow::anyhow!("bad sk length"))?;
        let signing = SigningKey::from_bytes(&sk_arr);
        let sig = signing.sign(msg);
        Ok(sig.to_bytes().to_vec())
    }

    fn hash_block_header(&self, header: &BlockHeader) -> BlockHash {
        let encoded = serde_json::to_vec(header).unwrap();
        blake3::hash(&encoded).into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarkProofWrapper {
    pub proof: Vec<u8>,
    pub public_result: u64,
}

#[derive(Debug, Error)]
pub enum StarkError {
    #[error("proving error: {0}")]
    Proving(String),
    #[error("verification error: {0}")]
    Verification(String),
    #[error("deserialization error: {0}")]
    Deserialize(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnarkProof {
    pub proof: Vec<u8>,
    pub public_inputs: Vec<u128>,
}

#[derive(Debug, Error)]
pub enum SnarkError {
    #[error("proving error: {0}")]
    Proving(String),
    #[error("verification error: {0}")]
    Verification(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

#[async_trait]
pub trait ZkStarkBackend: Send + Sync {
    fn prove_connection(&self, metadata: &ChainMetadata) -> std::result::Result<StarkProofWrapper, StarkError>;
    fn verify_connection(
        &self,
        proof: &StarkProofWrapper,
        metadata: &ChainMetadata,
    ) -> std::result::Result<(), StarkError>;
}

#[async_trait]
pub trait ZkSnarkBackend: Send + Sync {
    fn prove_message(&self, msg: &CrossChainMessage) -> std::result::Result<SnarkProof, SnarkError>;
    fn verify_message(
        &self,
        proof: &SnarkProof,
        msg: &CrossChainMessage,
    ) -> std::result::Result<(), SnarkError>;
}

// --- STARK backend (Winterfell Fibonacci example) ---
#[derive(Clone)]
struct FibAir {
    pub_inputs: FibPublicInputs,
    context: winterfell::AirContext<BaseElement>,
}

#[derive(Clone)]
struct FibPublicInputs {
    pub result: BaseElement,
}

impl ToElements<BaseElement> for FibPublicInputs {
    fn to_elements(&self) -> Vec<BaseElement> {
        vec![self.result]
    }
}

impl winterfell::Air for FibAir {
    type BaseField = BaseElement;
    type PublicInputs = FibPublicInputs;

    fn new(trace_info: winterfell::TraceInfo, pub_inputs: Self::PublicInputs, options: ProofOptions) -> Self {
        let degrees = vec![winterfell::TransitionConstraintDegree::new(1)];
        let context = winterfell::AirContext::new(trace_info, degrees, 1, options);
        Self { pub_inputs, context }
    }

    fn context(&self) -> &winterfell::AirContext<Self::BaseField> {
        &self.context
    }

    fn evaluate_transition<E: FieldElement<BaseField = BaseElement>>(
        &self,
        frame: &winterfell::EvaluationFrame<E>,
        _periodic_values: &[E],
        result: &mut [E],
    ) {
        let current = frame.current();
        let next = frame.next();
        result[0] = next[0] - (current[0] + current[1]);
    }

    fn get_assertions(&self) -> Vec<winterfell::Assertion<Self::BaseField>> {
        vec![
            winterfell::Assertion::single(0, 0, BaseElement::ONE),
            winterfell::Assertion::single(0, 1, BaseElement::ONE),
            winterfell::Assertion::single(self.context.trace_len() - 1, 0, self.pub_inputs.result),
        ]
    }
}

struct FibProver {
    options: ProofOptions,
}

impl FibProver {
    pub fn new(options: ProofOptions) -> Self {
        Self { options }
    }
}

impl Prover for FibProver {
    type BaseField = BaseElement;
    type Air = FibAir;
    type Trace = winterfell::TraceTable<Self::BaseField>;
    type HashFn = winterfell::crypto::hashers::Blake3_256<Self::BaseField>;
    type RandomCoin = DefaultRandomCoin<Self::HashFn>;

    fn get_pub_inputs(&self, trace: &Self::Trace) -> <Self::Air as winterfell::Air>::PublicInputs {
        let last = trace.length() - 1;
        FibPublicInputs {
            result: trace.get(last, 0),
        }
    }

    fn options(&self) -> &ProofOptions {
        &self.options
    }
}

fn build_fib_trace(steps: usize) -> winterfell::TraceTable<BaseElement> {
    let steps = steps.max(8);
    let mut col1 = vec![BaseElement::ZERO; steps];
    let mut col2 = vec![BaseElement::ZERO; steps];
    col1[0] = BaseElement::ONE;
    col2[0] = BaseElement::ONE;
    for i in 1..steps {
        col1[i] = col2[i - 1];
        col2[i] = col1[i - 1] + col2[i - 1];
    }
    winterfell::TraceTable::init(vec![col1, col2])
}

pub struct WinterfellBackend {
    options: ProofOptions,
}

impl WinterfellBackend {
    pub fn new() -> Self {
        let options = ProofOptions::new(32, 4, 0, winterfell::FieldExtension::None, 8, 256);
        Self { options }
    }
}

impl Default for WinterfellBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ZkStarkBackend for WinterfellBackend {
    fn prove_connection(&self, metadata: &ChainMetadata) -> std::result::Result<StarkProofWrapper, StarkError> {
        let target_steps = (metadata.latest_height as usize).max(16);
        let trace = build_fib_trace(target_steps);
        let prover = FibProver::new(self.options.clone());
        let proof = prover
            .prove(trace)
            .map_err(|e| StarkError::Proving(format!("{e:?}")))?;
        let result = fib_number(target_steps as u64);
        let bytes = proof.to_bytes();
        Ok(StarkProofWrapper {
            proof: bytes,
            public_result: result,
        })
    }

    fn verify_connection(
        &self,
        proof: &StarkProofWrapper,
        metadata: &ChainMetadata,
    ) -> std::result::Result<(), StarkError> {
        let steps = (metadata.latest_height as usize).max(16);
        let result = fib_number(steps as u64);
        if result != proof.public_result {
            return Err(StarkError::Verification("public result mismatch".into()));
        }
        let stark_proof =
            StarkProof::from_bytes(&proof.proof).map_err(|e| StarkError::Deserialize(format!("{e:?}")))?;
        let pub_inputs = FibPublicInputs {
            result: BaseElement::from(result),
        };
        type HashFn = winterfell::crypto::hashers::Blake3_256<BaseElement>;
        type Coin = DefaultRandomCoin<HashFn>;
        winterfell::verify::<FibAir, HashFn, Coin>(stark_proof, pub_inputs)
            .map_err(|e| StarkError::Verification(format!("{e:?}")))
    }
}

fn fib_number(n: u64) -> u64 {
    let mut a = 1u64;
    let mut b = 1u64;
    if n == 0 {
        return 0;
    }
    for _ in 1..n {
        let tmp = a + b;
        a = b;
        b = tmp;
    }
    b
}

// --- SNARK backend (Groth16 demo) ---
#[derive(Clone)]
struct SumCircuit<F: PrimeField> {
    a: F,
    b: F,
    c: F,
}

impl<F: PrimeField> ConstraintSynthesizer<F> for SumCircuit<F> {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> std::result::Result<(), SynthesisError> {
        let a_var = cs.new_witness_variable(|| Ok(self.a))?;
        let b_var = cs.new_witness_variable(|| Ok(self.b))?;
        let c_var = cs.new_input_variable(|| Ok(self.c))?;
        cs.enforce_constraint(
            ark_relations::r1cs::LinearCombination::from(a_var),
            ark_relations::r1cs::LinearCombination::from(b_var),
            ark_relations::r1cs::LinearCombination::from(c_var),
        )?;
        Ok(())
    }
}

pub struct Groth16Backend {
    pk: ProvingKey<Bls12_381>,
    vk: ark_groth16::PreparedVerifyingKey<Bls12_381>,
}

impl Groth16Backend {
    pub fn new() -> Result<Self, SnarkError> {
        let circuit = SumCircuit::<ark_bls12_381::Fr> {
            a: ark_bls12_381::Fr::from(1u64),
            b: ark_bls12_381::Fr::from(1u64),
            c: ark_bls12_381::Fr::from(1u64),
        };
        let mut rng = thread_rng();
        let params = Groth16::<Bls12_381, LibsnarkReduction>::generate_random_parameters_with_reduction(circuit, &mut rng)
            .map_err(|e| SnarkError::Proving(e.to_string()))?;
        let pvk = prepare_verifying_key(&params.vk);
        Ok(Self { pk: params, vk: pvk })
    }
}

impl Default for Groth16Backend {
    fn default() -> Self {
        Self::new().expect("groth16 init")
    }
}

#[async_trait]
impl ZkSnarkBackend for Groth16Backend {
    fn prove_message(&self, msg: &CrossChainMessage) -> std::result::Result<SnarkProof, SnarkError> {
        let hash = blake3::hash(serde_json::to_string(msg).unwrap().as_bytes());
        let a_val = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().unwrap());
        let a = ark_bls12_381::Fr::from(a_val);
        let b = ark_bls12_381::Fr::from(1u64);
        let c = a + b;
        let circuit = SumCircuit { a, b, c };
        let mut rng = thread_rng();
        let proof = Groth16::<Bls12_381, LibsnarkReduction>::create_random_proof_with_reduction(circuit, &self.pk, &mut rng)
            .map_err(|e| SnarkError::Proving(e.to_string()))?;
        let mut proof_bytes = Vec::new();
        proof
            .serialize_uncompressed(&mut proof_bytes)
            .map_err(|e| SnarkError::Serialization(e.to_string()))?;
        Ok(SnarkProof {
            proof: proof_bytes,
            public_inputs: vec![a_val as u128, 1, (a_val + 1) as u128],
        })
    }

    fn verify_message(
        &self,
        proof: &SnarkProof,
        msg: &CrossChainMessage,
    ) -> std::result::Result<(), SnarkError> {
        let hash = blake3::hash(serde_json::to_string(msg).unwrap().as_bytes());
        let a_val = u64::from_le_bytes(hash.as_bytes()[0..8].try_into().unwrap());
        let a = ark_bls12_381::Fr::from(a_val);
        let b = ark_bls12_381::Fr::from(1u64);
        let c = a + b;
        let public_inputs = vec![a, b, c];
        let mut cursor = &proof.proof[..];
        let proof: Proof<Bls12_381> = Proof::deserialize_uncompressed(&mut cursor)
            .map_err(|e| SnarkError::Serialization(e.to_string()))?;
        let ok = Groth16::<Bls12_381, LibsnarkReduction>::verify_proof(&self.vk, &proof, &public_inputs)
            .map_err(|e| SnarkError::Verification(e.to_string()))?;
        if ok {
            Ok(())
        } else {
            Err(SnarkError::Verification("proof invalid".into()))
        }
    }
}

pub fn address_to_string(addr: &Address) -> String {
    bs58::encode(addr).into_string()
}

pub fn address_from_string(s: &str) -> Result<Address> {
    let bytes = bs58::decode(s).into_vec()?;
    if bytes.len() != 32 {
        return Err(anyhow::anyhow!("invalid address length"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxid_core::{BlockHeader, ChainMetadata, CrossChainMessage};

    #[test]
    fn sign_and_verify() {
        let kp = generate_ed25519();
        let provider = DefaultCryptoProvider::new();
        let msg = b"hello world";
        let sig = provider.sign_message(&kp.secret_key, msg).unwrap();
        assert!(provider.verify_signature(&kp.public_key, msg, &sig).unwrap());
    }

    #[test]
    fn groth16_roundtrip() {
        let backend = Groth16Backend::new().unwrap();
        let msg = CrossChainMessage {
            id: uuid::Uuid::new_v4(),
            source: "a".into(),
            dest: "b".into(),
            payload: serde_json::json!({"hello": "world"}),
            nonce: 1,
            timestamp: 0,
        };
        let proof = backend.prove_message(&msg).unwrap();
        backend.verify_message(&proof, &msg).unwrap();
    }

    #[test]
    fn winterfell_demo() {
        let backend = WinterfellBackend::new();
        let meta = chain_metadata("demo".into(), "http://localhost".into());
        let proof = backend.prove_connection(&meta).unwrap();
        backend.verify_connection(&proof, &meta).unwrap();
    }
}
