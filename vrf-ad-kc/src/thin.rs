// Copyright (c) 2019-2020 Web 3 Foundation
//
// Authors:
// - Jeffrey Burdges <jeff@web3.foundation>

//! ### Thin VRF routines

use ark_ec::{AffineRepr, CurveGroup};

use rand_core::{RngCore,CryptoRng};

use crate::{
    SigningTranscript, 
    flavor::{Flavor, InnerFlavor, Witness, Signature},
    keys::{PublicKey, SecretKey},
    error::{SignatureResult, SignatureError},
    vrf::{self, VrfInput, VrfInOut},
};

use core::borrow::{Borrow,BorrowMut};


/// Thin VRF flavor
#[derive(Clone)]
pub struct ThinVrf<C: AffineRepr> {
    pub keying_base: C,
}


impl<C: AffineRepr> Flavor for ThinVrf<C> {
    type ScalarField = <C as AffineRepr>::ScalarField;
    type KeyAffine = C;
    type PreOutAffine = C;

    fn keying_base(&self) -> &C { &self.keying_base }
}

impl<C: AffineRepr> InnerFlavor for ThinVrf<C> {
    type KeyCommitment = ();
    type Scalars = <C as AffineRepr>::ScalarField;
    type Affines = C;
}


impl<C: AffineRepr> ThinVrf<C> {
    /// Attach a public key to its base point.
    fn schnorr_io(&self, public: &PublicKey<C>) -> VrfInOut<C> {
        VrfInOut {
            input: VrfInput( self.keying_base.clone() ),
            preoutput: vrf::VrfPreOut( public.0.clone() ),
        }
    }

    /// Merge VRF operation which incorporates the public key.
    fn thin_vrf_merge<T,B>(&self, t: &mut T, public: &PublicKey<C>, ios: &[B]) -> VrfInOut<C> 
    where T: SigningTranscript+Clone, B: Borrow<VrfInOut<C>>,
    {
        let io = self.schnorr_io(public);
        // Append base too since we're being so polymorphic.
        t.append(b"PublicKey",&io);
        if ios.len() == 0 { return io }
        t.append_u64(b"IOs",ios.len() as u64); 
        t.append_slice(b"VrfInOut", ios);
        vrf::vrfs_delinearize( t, ios.iter().map(|io| io.borrow()).chain([ &io ]) )
    }
}


// --- Sign --- //

impl<C: AffineRepr> SecretKey<ThinVrf<C>> {
    pub(crate) fn new_thin_witness<T,R>(
        &self, t: &T, input: &VrfInput<C>, rng: &mut R
    ) -> Witness<ThinVrf<C>>
    where T: SigningTranscript, R: RngCore+CryptoRng
    {
        let k: [<C as AffineRepr>::ScalarField; 1]
         = t.witnesses(b"MakeWitness", &[&self.nonce_seed], rng);
        let k = k[0];
        let r = input.0.mul(k).into_affine();
        Witness { r, k }
    }

    /// Sign thin VRF signature
    /// 
    /// If `ios = &[]` this reduces to a Schnorr signature.
    pub fn sign_thin_vrf<T,B,R>(
        &self, mut t: B, ios: &[VrfInOut<C>], rng: &mut R
    ) -> Signature<ThinVrf<C>>
    where T: SigningTranscript+Clone, B: BorrowMut<T>, R: RngCore+CryptoRng
    {
        let t = t.borrow_mut();
        let io = self.flavor.thin_vrf_merge(t, self.as_publickey(), ios);
        // Allow derandomization by constructing witness late.
        self.new_thin_witness(t,&io.input,rng).sign_final(t,self)
    }
}

impl<C: AffineRepr> Witness<ThinVrf<C>> {
    /// Complete Schnorr-like signature.
    /// 
    /// Assumes we already hashed public key, `VrfInOut`s, etc.
    pub(crate) fn sign_final<T: SigningTranscript>(
        self, t: &mut T, secret: &SecretKey<ThinVrf<C>>
    ) -> Signature<ThinVrf<C>> {
        let Witness { r, k } = self;
        t.append(b"Witness", &r);
        let c: <C as AffineRepr>::ScalarField = t.challenge(b"ThinVrfChallenge");
        let s = k + c * secret.key;
        // k.zeroize();
        Signature { compk: (), r, s }
    }
}

/*
impl<C: AffineRepr> Valid for Signature<ThinVrf<C>> {
    fn check(&self) -> Result<(), SerializationError> {
        if self.is_on_curve() && self.is_in_correct_subgroup_assuming_on_curve() {
            Ok(())
        } else {
            Err(SerializationError::InvalidData)
        }
    }
}
*/


// --- Verify --- //

impl<C: AffineRepr> ThinVrf<C> {
    /// Verify thin VRF signature 
    /// 
    /// If `ios = &[]` this reduces to a Schnorr signature.
    pub fn verify_thin_vrf<'a,T,B>(
        &self,
        mut t: B,
        public: &PublicKey<C>,
        ios: &'a [VrfInOut<C>],
        signature: &Signature<ThinVrf<C>>,
    ) -> SignatureResult<&'a [VrfInOut<C>]>
    where T: SigningTranscript+Clone, B: BorrowMut<T>
    {
        let t = t.borrow_mut();
        let io = self.thin_vrf_merge(t, public, ios);

        // verify_final
        t.append(b"Witness", &signature.r);
        let c: <C as AffineRepr>::ScalarField = t.challenge(b"ThinVrfChallenge");

        let lhs = io.input.0.mul(signature.s);
        let rhs = signature.r.into_group() + io.preoutput.0.mul(c);
        if crate::eq_mod_small_cofactor_projective(&lhs, &rhs) {
            Ok(ios)
        } else {
            Err(SignatureError::Invalid)
        }
    }
}

