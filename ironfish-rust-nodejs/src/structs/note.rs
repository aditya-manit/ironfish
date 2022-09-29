/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use napi::bindgen_prelude::*;
use napi_derive::napi;

use ironfish_rust::{
    note::Memo, proofs::notes::spendable_note::SpendableNote, AssetType, Note, SaplingKey,
};

#[napi(js_name = "Note")]
pub struct NativeNote {
    pub(crate) note: Note,
}

#[napi]
impl NativeNote {
    #[napi(constructor)]
    pub fn new(
        owner: String,
        value: BigInt,
        memo: String,
        asset_identifier: Buffer,
    ) -> Result<Self> {
        let value_u64 = value.get_u64().1;

        let owner_address = ironfish_rust::PublicAddress::from_hex(&owner)
            .map_err(|err| Error::from_reason(err.to_string()))?;
        let mut identifier = [0; 32];
        identifier.copy_from_slice(&asset_identifier);
        let asset_type = AssetType::from_identifier(&identifier)
            .map_err(|err| Error::from_reason(err.to_string()))?;
        Ok(NativeNote {
            note: Note::new(owner_address, value_u64, Memo::from(memo), asset_type),
        })
    }

    #[napi(factory)]
    pub fn deserialize(bytes: Buffer) -> Result<Self> {
        let note = Note::read(bytes.as_ref()).map_err(|err| Error::from_reason(err.to_string()))?;

        Ok(NativeNote { note })
    }

    #[napi]
    pub fn serialize(&self) -> Result<Buffer> {
        let mut arr: Vec<u8> = vec![];
        self.note
            .write(&mut arr)
            .map_err(|err| Error::from_reason(err.to_string()))?;

        Ok(Buffer::from(arr))
    }

    /// Value this note represents.
    #[napi]
    pub fn value(&self) -> u64 {
        self.note.value()
    }

    /// Arbitrary note the spender can supply when constructing a spend so the
    /// receiver has some record from whence it came.
    /// Note: While this is encrypted with the output, it is not encoded into
    /// the proof in any way.
    #[napi]
    pub fn memo(&self) -> String {
        self.note.memo().to_string()
    }

    /// Compute the nullifier for this note, given the private key of its owner.
    ///
    /// The nullifier is a series of bytes that is published by the note owner
    /// only at the time the note is spent. This key is collected in a massive
    /// 'nullifier set', preventing double-spend.
    #[napi]
    pub fn nullifier(&self, owner_private_key: String, position: BigInt) -> Result<Buffer> {
        let position_u64 = position.get_u64().1;

        let private_key = SaplingKey::from_hex(&owner_private_key)
            .map_err(|err| Error::from_reason(err.to_string()))?;

        let nullifier: &[u8] = &self.note.nullifier(&private_key, position_u64).0;

        Ok(Buffer::from(nullifier))
    }

    #[napi]
    pub fn get_default_asset_identifier() -> Buffer {
        Buffer::from(AssetType::default().get_identifier().to_vec())
    }
}
