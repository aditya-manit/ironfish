/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::{errors::IronfishError, util::str_to_array};

use super::{
    keys::{IncomingViewKey, PublicAddress, SaplingKey},
    serializing::{aead, read_scalar, scalar_to_bytes},
};
use crate::keys::PUBLIC_KEY_GENERATOR;
use bls12_381::Scalar;
use byteorder::{ByteOrder, LittleEndian, ReadBytesExt, WriteBytesExt};
use ff::{Field, PrimeField};
use ironfish_zkp::{Nullifier, Rseed, SaplingNote};
use jubjub::SubgroupPoint;
use rand::thread_rng;

use std::{fmt, io, io::Read};

pub const ENCRYPTED_NOTE_SIZE: usize = 72;

/// Memo field on a Note. Used to encode transaction IDs or other information
/// about the transaction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Memo(pub [u8; 32]);

impl From<&str> for Memo {
    fn from(string: &str) -> Self {
        let memo_bytes = str_to_array(string);
        Memo(memo_bytes)
    }
}

impl From<String> for Memo {
    fn from(string: String) -> Self {
        Memo::from(string.as_str())
    }
}

impl fmt::Display for Memo {
    /// This can be lossy because it assumes that the
    /// memo is in valid UTF-8 format.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

/// A note (think bank note) represents a value in the owner's "account".
/// When spending, proof that the note exists in the tree needs to be provided,
/// along with a nullifier key that is made public so the owner cannot attempt
/// to spend that note again.447903
///
/// When receiving funds, a new note needs to be created for the new owner
/// to hold those funds.
#[derive(Clone)]
pub struct Note {
    /// A public address for the owner of the note. One owner can have multiple public addresses,
    /// each associated with a different diversifier.
    pub(crate) owner: PublicAddress,

    /// Value this note represents.
    pub(crate) value: u64,

    /// A random value generated when the note is constructed.
    /// This helps create zero knowledge around the note,
    /// allowing the owner to prove they have the note without revealing
    /// anything else about it.
    pub(crate) randomness: jubjub::Fr,

    /// Arbitrary note the spender can supply when constructing a spend so the
    /// receiver has some record from whence it came.
    /// Note: While this is encrypted with the output, it is not encoded into
    /// the proof in any way.
    pub(crate) memo: Memo,
}

impl<'a> Note {
    /// Construct a new Note.
    pub fn new(owner: PublicAddress, value: u64, memo: impl Into<Memo>) -> Self {
        let randomness: jubjub::Fr = jubjub::Fr::random(thread_rng());

        Self {
            owner,
            value,
            randomness,
            memo: memo.into(),
        }
    }

    /// Read a note from the given stream IN PLAINTEXT.
    ///
    /// You probably don't want to use this unless you are transmitting
    /// across nodejs threads in memory.
    pub fn read<R: io::Read>(mut reader: R) -> Result<Self, IronfishError> {
        let owner = PublicAddress::read(&mut reader)?;
        let value = reader.read_u64::<LittleEndian>()?;
        let randomness: jubjub::Fr = read_scalar(&mut reader)?;

        let mut memo = Memo::default();
        reader.read_exact(&mut memo.0)?;

        Ok(Self {
            owner,
            value,
            randomness,
            memo,
        })
    }

    /// Write the note to the given stream IN PLAINTEXT.
    ///
    /// This should generally never be used to serialize to disk or the network.
    /// It is primarily added as a device for transmitting the note across
    /// thread boundaries.
    pub fn write<W: io::Write>(&self, mut writer: &mut W) -> Result<(), IronfishError> {
        self.owner.write(&mut writer)?;
        writer.write_u64::<LittleEndian>(self.value)?;
        writer.write_all(self.randomness.to_repr().as_ref())?;
        writer.write_all(&self.memo.0)?;

        Ok(())
    }

    /// Create a note from its encrypted representation, given the owner's
    /// view key.
    ///
    /// The note is stored on the [`crate::outputs::OutputDescription`] in
    /// encrypted form. The spender encrypts it when they construct the output
    /// using a shared secret derived from the owner's public key.
    ///
    /// This function allows the owner to decrypt the note using the derived
    /// shared secret and their own view key.
    pub fn from_owner_encrypted(
        owner_view_key: &'a IncomingViewKey,
        shared_secret: &[u8; 32],
        encrypted_bytes: &[u8; ENCRYPTED_NOTE_SIZE + aead::MAC_SIZE],
    ) -> Result<Self, IronfishError> {
        let (randomness, value, memo) = Note::decrypt_note_parts(shared_secret, encrypted_bytes)?;
        let owner = owner_view_key.public_address();

        Ok(Note {
            owner,
            value,
            randomness,
            memo,
        })
    }

    /// Create a note from its encrypted representation, given the spender's
    /// view key.
    ///
    /// The note is stored on the [`crate::outputs::OutputDescription`] in
    /// encrypted form. The spender encrypts it when they construct the output
    /// using a shared secret derived from the owner's public key.
    ///
    /// This function allows the owner to decrypt the note using the derived
    /// shared secret and their own view key.
    pub(crate) fn from_spender_encrypted(
        transmission_key: SubgroupPoint,
        shared_secret: &[u8; 32],
        encrypted_bytes: &[u8; ENCRYPTED_NOTE_SIZE + aead::MAC_SIZE],
    ) -> Result<Self, IronfishError> {
        let (randomness, value, memo) = Note::decrypt_note_parts(shared_secret, encrypted_bytes)?;

        let owner = PublicAddress { transmission_key };

        Ok(Note {
            owner,
            value,
            randomness,
            memo,
        })
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn memo(&self) -> Memo {
        self.memo
    }

    pub fn owner(&self) -> PublicAddress {
        self.owner
    }

    /// Send encrypted form of the note, which is what gets publicly stored on
    /// the tree. Only someone with the incoming viewing key for the note can
    /// actually read the contents.
    pub fn encrypt(&self, shared_secret: &[u8; 32]) -> [u8; ENCRYPTED_NOTE_SIZE + aead::MAC_SIZE] {
        let mut bytes_to_encrypt = [0; ENCRYPTED_NOTE_SIZE];
        bytes_to_encrypt[..32].clone_from_slice(self.randomness.to_repr().as_ref());

        LittleEndian::write_u64_into(&[self.value], &mut bytes_to_encrypt[32..40]);
        bytes_to_encrypt[40..].copy_from_slice(&self.memo.0[..]);
        let mut encrypted_bytes = [0; ENCRYPTED_NOTE_SIZE + aead::MAC_SIZE];
        aead::encrypt(shared_secret, &bytes_to_encrypt, &mut encrypted_bytes);

        encrypted_bytes
    }

    /// Compute the nullifier for this note, given the private key of its owner.
    ///
    /// The nullifier is a series of bytes that is published by the note owner
    /// only at the time the note is spent. This key is collected in a massive
    /// 'nullifier set', preventing double-spend.
    pub fn nullifier(&self, private_key: &SaplingKey, position: u64) -> Nullifier {
        self.sapling_note()
            .nf(&private_key.sapling_viewing_key(), position)
    }

    /// Get the commitment hash for this note. This encapsulates all the values
    /// in the note, including the randomness and converts them to a byte
    /// format. This hash is what gets used for the leaf nodes in a Merkle Tree.
    pub fn commitment(&self) -> [u8; 32] {
        scalar_to_bytes(&self.commitment_point())
    }

    /// Compute the commitment of this note. This is essentially a hash of all
    /// the note values, including randomness.
    ///
    /// The owner can publish this value to commit to the fact that the note
    /// exists, without revealing any of the values on the note until later.
    pub(crate) fn commitment_point(&self) -> Scalar {
        self.sapling_note().cmu()
    }

    /// Verify that the note's commitment matches the one passed in
    pub(crate) fn verify_commitment(&self, commitment: Scalar) -> Result<(), IronfishError> {
        if commitment == self.commitment_point() {
            Ok(())
        } else {
            Err(IronfishError::InvalidCommitment)
        }
    }

    fn decrypt_note_parts(
        shared_secret: &[u8; 32],
        encrypted_bytes: &[u8; ENCRYPTED_NOTE_SIZE + aead::MAC_SIZE],
    ) -> Result<(jubjub::Fr, u64, Memo), IronfishError> {
        let mut plaintext_bytes = [0; ENCRYPTED_NOTE_SIZE];
        aead::decrypt(shared_secret, encrypted_bytes, &mut plaintext_bytes)?;

        let mut reader = plaintext_bytes[..].as_ref();

        let randomness: jubjub::Fr = read_scalar(&mut reader)?;
        let value = reader.read_u64::<LittleEndian>()?;

        let mut memo = Memo::default();
        reader.read_exact(&mut memo.0)?;

        Ok((randomness, value, memo))
    }

    /// The zcash_primitives version of the Note API is kind of klunky with
    /// annoying variable names and exposed values, but it contains the methods
    /// used to calculate nullifier and commitment.
    ///
    /// This is somewhat suboptimal with extra calculations and bytes being
    /// passed around. I'm not worried about it yet, since only notes actively
    /// being spent have to create these.
    fn sapling_note(&self) -> SaplingNote {
        SaplingNote {
            value: self.value,
            g_d: PUBLIC_KEY_GENERATOR,
            pk_d: self.owner.transmission_key,
            rseed: Rseed::BeforeZip212(self.randomness),
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Memo, Note};
    use crate::keys::{shared_secret, SaplingKey};

    #[test]
    fn test_plaintext_serialization() {
        let owner_key: SaplingKey = SaplingKey::generate_key();
        let public_address = owner_key.public_address();
        let note = Note::new(public_address, 42, "serialize me");
        let mut serialized = Vec::new();
        note.write(&mut serialized)
            .expect("Should serialize cleanly");

        let note2 = Note::read(&serialized[..]).expect("It should deserialize cleanly");
        assert_eq!(note2.owner.public_address(), note.owner.public_address());
        assert_eq!(note2.value, 42);
        assert_eq!(note2.randomness, note.randomness);
        assert_eq!(note2.memo, note.memo);

        let mut serialized2 = Vec::new();
        note2
            .write(&mut serialized2)
            .expect("Should still serialize cleanly");
        assert_eq!(serialized, serialized2)
    }

    #[test]
    fn test_note_encryption() {
        let owner_key: SaplingKey = SaplingKey::generate_key();
        let public_address = owner_key.public_address();
        let (dh_secret, dh_public) = public_address.generate_diffie_hellman_keys();
        let public_shared_secret =
            shared_secret(&dh_secret, &public_address.transmission_key, &dh_public);
        let note = Note::new(public_address, 42, "");
        let encryption_result = note.encrypt(&public_shared_secret);

        let private_shared_secret = owner_key.incoming_view_key().shared_secret(&dh_public);
        assert_eq!(private_shared_secret, public_shared_secret);

        let restored_note = Note::from_owner_encrypted(
            owner_key.incoming_view_key(),
            &private_shared_secret,
            &encryption_result,
        )
        .expect("Should be able to decrypt bytes");
        assert!(
            restored_note.owner.public_address().as_ref() == note.owner.public_address().as_ref()
        );
        assert!(note.value == restored_note.value);
        assert!(note.randomness == restored_note.randomness);
        assert!(note.memo == restored_note.memo);

        let spender_decrypted = Note::from_spender_encrypted(
            note.owner.transmission_key,
            &public_shared_secret,
            &encryption_result,
        )
        .expect("Should be able to load from transmission key");
        assert!(
            spender_decrypted.owner.public_address().as_ref()
                == note.owner.public_address().as_ref()
        );
        assert!(note.value == spender_decrypted.value);
        assert!(note.randomness == spender_decrypted.randomness);
        assert!(note.memo == spender_decrypted.memo);
    }

    #[test]
    fn construct_memo_from_string() {
        let memo = Memo::from("a memo");
        assert_eq!(&memo.0[..6], b"a memo");
        let string = "a memo".to_string();
        let memo = Memo::from(&*string);
        assert_eq!(&memo.0[..6], b"a memo");
        let memo = Memo::from(string);
        assert_eq!(&memo.0[..6], b"a memo");
    }
}
