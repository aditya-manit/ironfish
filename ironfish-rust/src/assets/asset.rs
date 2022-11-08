/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
use crate::{errors::IronfishError, util::str_to_array, PublicAddress};
use group::GroupEncoding;
use ironfish_zkp::{
    constants::{ASSET_IDENTIFIER_LENGTH, VALUE_COMMITMENT_GENERATOR_PERSONALIZATION},
    group_hash,
};
use std::{io::Write, slice};

#[allow(dead_code)]
pub type AssetIdentifier = [u8; ASSET_IDENTIFIER_LENGTH];

pub const NATIVE_ASSET: AssetIdentifier = [
    215, 200, 103, 6, 245, 129, 122, 167, 24, 205, 28, 250, 208, 50, 51, 188, 214, 74, 119, 137,
    253, 148, 34, 211, 177, 122, 246, 130, 58, 126, 106, 198,
];

const IDENTIFIER_PREIMAGE_LENGTH: usize = 32 // name
    + 32 // chain
    + 32 // network
    + 32 // token_identifier
    + 43 // owner public address
    + 1; // nonce

/// Describes all the fields necessary for creating and transacting with an
/// asset on the Iron Fish network
#[allow(dead_code)]
pub struct Asset {
    /// Name of the asset
    name: [u8; 32],

    /// Chain on the network the asset originated from (ex. Ropsten)
    chain: [u8; 32],

    /// Network the asset originated from (ex. Ethereum)
    network: [u8; 32],

    /// Identifier field for bridged asset address, or if a native custom asset, random bytes.
    token_identifier: [u8; 32],

    /// The owner who created the asset. Has permissions to mint
    owner: PublicAddress,

    /// The random byte used to ensure we get a valid asset identifier
    nonce: u8,

    /// Unique byte array which is a hash of all of the identifying fields for
    /// an asset
    identifier: AssetIdentifier,
}

impl Asset {
    /// Create a new AssetType from a public address, name, chain, and network
    #[allow(dead_code)]
    pub fn new(
        owner: PublicAddress,
        name: &str,
        chain: &str,
        network: &str,
        token_identifier: &str,
    ) -> Result<Asset, IronfishError> {
        let name_bytes = str_to_array(name);
        let chain_bytes = str_to_array(chain);
        let network_bytes = str_to_array(network);
        let token_identifier_bytes = str_to_array(token_identifier);

        let mut nonce = 0u8;
        loop {
            if let Ok(asset_info) = Asset::new_with_nonce(
                owner,
                name_bytes,
                chain_bytes,
                network_bytes,
                token_identifier_bytes,
                nonce,
            ) {
                return Ok(asset_info);
            }

            nonce = nonce.checked_add(1).ok_or(IronfishError::RandomnessError)?;
        }
    }

    #[allow(dead_code)]
    fn new_with_nonce(
        owner: PublicAddress,
        name: [u8; 32],
        chain: [u8; 32],
        network: [u8; 32],
        token_identifier: [u8; 32],
        nonce: u8,
    ) -> Result<Asset, IronfishError> {
        let mut preimage = Vec::with_capacity(IDENTIFIER_PREIMAGE_LENGTH);
        preimage.write_all(&owner.public_address())?;
        preimage.write_all(&name)?;
        preimage.write_all(&chain)?;
        preimage.write_all(&network)?;
        preimage.write_all(&token_identifier)?;
        preimage.write_all(slice::from_ref(&nonce))?;

        // Check that this is valid as a value commitment generator point
        if let Some(generator_point) =
            group_hash(&preimage, VALUE_COMMITMENT_GENERATOR_PERSONALIZATION)
        {
            Ok(Asset {
                owner,
                name,
                chain,
                network,
                token_identifier,
                nonce,
                identifier: generator_point.to_bytes(),
            })
        } else {
            Err(IronfishError::InvalidAssetIdentifier)
        }
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    #[allow(dead_code)]
    pub fn public_address(&self) -> &PublicAddress {
        &self.owner
    }

    #[allow(dead_code)]
    pub fn nonce(&self) -> &u8 {
        &self.nonce
    }

    #[allow(dead_code)]
    pub fn identifier(&self) -> &AssetIdentifier {
        &self.identifier
    }
}

#[cfg(test)]
mod test {
    use group::GroupEncoding;
    use ironfish_zkp::constants::VALUE_COMMITMENT_VALUE_GENERATOR;

    use crate::{util::str_to_array, PublicAddress, SaplingKey};

    use super::{Asset, NATIVE_ASSET};

    #[test]
    fn test_asset_new_with_nonce() {
        let owner = PublicAddress::new(&[
            19, 26, 159, 204, 98, 253, 225, 73, 168, 125, 3, 240, 3, 129, 255, 146, 50, 134, 44,
            84, 181, 195, 50, 249, 78, 128, 228, 152, 239, 10, 106, 10, 27, 58, 155, 162, 114, 133,
            17, 48, 177, 29, 72,
        ])
        .expect("can create a deterministic public address");
        let name = str_to_array("name");
        let chain = str_to_array("chain");
        let network = str_to_array("network");
        let token_identifier = str_to_array("token identifier");
        let nonce = 2;

        let asset = Asset::new_with_nonce(owner, name, chain, network, token_identifier, nonce)
            .expect("can create an asset");

        assert_eq!(asset.owner, owner);
        assert_eq!(asset.name, name);
        assert_eq!(asset.chain, chain);
        assert_eq!(asset.network, network);
        assert_eq!(asset.token_identifier, token_identifier);
        assert_eq!(asset.nonce, nonce);
        assert_eq!(
            asset.identifier,
            [
                239, 38, 106, 64, 62, 130, 45, 125, 77, 114, 12, 122, 9, 173, 248, 164, 86, 58,
                244, 54, 238, 165, 86, 164, 31, 98, 78, 192, 15, 94, 154, 25
            ],
        );
    }

    #[test]
    fn test_asset_new() {
        let key = SaplingKey::generate_key();
        let owner = key.generate_public_address();
        let name = "name";
        let chain = "chain";
        let network = "network";
        let token_identifier = "token identifier";

        let asset =
            Asset::new(owner, name, chain, network, token_identifier).expect("can create an asset");

        assert_eq!(asset.owner, owner);
        assert_eq!(asset.name, str_to_array(name));
        assert_eq!(asset.chain, str_to_array(chain));
        assert_eq!(asset.network, str_to_array(network));
        assert_eq!(asset.token_identifier, str_to_array(token_identifier));
    }

    #[test]
    fn test_asset_native_identifier() {
        // Native asset uses the original value commitment generator, no
        // particular reason other than it is easier to think about this way.
        assert_eq!(NATIVE_ASSET, VALUE_COMMITMENT_VALUE_GENERATOR.to_bytes());
    }
}
