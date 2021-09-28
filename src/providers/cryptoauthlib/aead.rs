// Copyright 2021 Contributors to the Parsec project.
// SPDX-License-Identifier: Apache-2.0
// CAL supports CCM with:
// - tag lenght must be <4,16> and must be odd number
// - nonce lenght must be <7,13>
// CAL supports GCM with:
// - tag lenght must be <12,16>
// - nonce lenght must be <7,13>

use super::Provider;
use crate::authenticators::ApplicationName;
use crate::key_info_managers::KeyTriple;
use log::error;
use parsec_interface::operations::psa_algorithm::{Aead, AeadWithDefaultLengthTag};
use parsec_interface::operations::{psa_aead_decrypt, psa_aead_encrypt};
use parsec_interface::requests::{ProviderId, ResponseStatus, Result};

const DEFAULT_TAG_LENGTH: usize = 16;

pub fn get_tag_length(alg: &Aead) -> Option<usize> {
    match alg {
        Aead::AeadWithDefaultLengthTag(AeadWithDefaultLengthTag::Ccm) => Some(DEFAULT_TAG_LENGTH),
        Aead::AeadWithDefaultLengthTag(AeadWithDefaultLengthTag::Gcm) => Some(DEFAULT_TAG_LENGTH),
        Aead::AeadWithShortenedTag {
            aead_alg: AeadWithDefaultLengthTag::Ccm,
            tag_length,
        } => Some(*tag_length),
        Aead::AeadWithShortenedTag {
            aead_alg: AeadWithDefaultLengthTag::Gcm,
            tag_length,
        } => Some(*tag_length),
        _ => None,
    }
}

pub fn is_ccm_selected(alg: &Aead) -> bool {
    matches!(
        alg,
        Aead::AeadWithDefaultLengthTag(AeadWithDefaultLengthTag::Ccm)
            | Aead::AeadWithShortenedTag {
                aead_alg: AeadWithDefaultLengthTag::Ccm,
                ..
            }
    )
}

impl Provider {
    pub(super) fn psa_aead_encrypt_internal(
        &self,
        app_name: ApplicationName,
        op: psa_aead_encrypt::Operation,
    ) -> Result<psa_aead_encrypt::Result> {
        match get_tag_length(&op.alg) {
            Some(tag_length) => {
                let key_triple =
                    KeyTriple::new(app_name, ProviderId::CryptoAuthLib, op.key_name.clone());
                let key_id = self.key_info_store.get_key_id::<u8>(&key_triple)?;
                let key_attributes = self.key_info_store.get_key_attributes(&key_triple)?;
                op.validate(key_attributes)?;

                let aead_param_gcm = rust_cryptoauthlib::AeadParam {
                    nonce: op.nonce.to_vec(),
                    tag_length: Some(tag_length as u8),
                    additional_data: Some(op.additional_data.to_vec()),
                    ..Default::default()
                };

                let aead_param_ccc = aead_param_gcm.clone();
                let mut aead_algorithm = rust_cryptoauthlib::AeadAlgorithm::Gcm(aead_param_gcm);
                if is_ccm_selected(&op.alg) {
                    aead_algorithm = rust_cryptoauthlib::AeadAlgorithm::Ccm(aead_param_ccc);
                }

                let mut plaintext = op.plaintext.to_vec();

                match self
                    .device
                    .aead_encrypt(aead_algorithm, key_id, &mut plaintext)
                {
                    Ok(tag) => {
                        plaintext.extend(tag);

                        Ok(psa_aead_encrypt::Result {
                            ciphertext: plaintext.into(),
                        })
                    }
                    Err(error) => {
                        error!("aead_encrypt failed CAL error {}.", error);
                        Err(ResponseStatus::PsaErrorGenericError)
                    }
                }
            }
            None => {
                error!("aead_encrypt failed, algorithm not supported");
                Err(ResponseStatus::PsaErrorNotSupported)
            }
        }
    }

    pub(super) fn psa_aead_decrypt_internal(
        &self,
        app_name: ApplicationName,
        op: psa_aead_decrypt::Operation,
    ) -> Result<psa_aead_decrypt::Result> {
        match get_tag_length(&op.alg) {
            Some(tag_length) => {
                let key_triple =
                    KeyTriple::new(app_name, ProviderId::CryptoAuthLib, op.key_name.clone());
                let key_id = self.key_info_store.get_key_id::<u8>(&key_triple)?;
                let key_attributes = self.key_info_store.get_key_attributes(&key_triple)?;
                op.validate(key_attributes)?;

                let mut ciphertext: Vec<_> = op.ciphertext.to_vec();
                let tag: Vec<_> = ciphertext
                    .drain((ciphertext.len() - tag_length)..)
                    .collect();

                let aead_param_gcm = rust_cryptoauthlib::AeadParam {
                    nonce: op.nonce.to_vec(),
                    tag: Some(tag),
                    additional_data: Some(op.additional_data.to_vec()),
                    ..Default::default()
                };

                let aead_param_ccc = aead_param_gcm.clone();
                let mut aead_algorithm = rust_cryptoauthlib::AeadAlgorithm::Gcm(aead_param_gcm);
                if is_ccm_selected(&op.alg) {
                    aead_algorithm = rust_cryptoauthlib::AeadAlgorithm::Ccm(aead_param_ccc);
                }

                match self
                    .device
                    .aead_decrypt(aead_algorithm, key_id, &mut ciphertext)
                {
                    Ok(true) => Ok(psa_aead_decrypt::Result {
                        plaintext: ciphertext.into(),
                    }),
                    Ok(false) => {
                        error!("aead_decrypt authentication failed");
                        Err(ResponseStatus::PsaErrorGenericError)
                    }
                    Err(error) => {
                        error!("aead_decrypt error {}", error);
                        Err(ResponseStatus::PsaErrorGenericError)
                    }
                }
            }

            None => {
                error!("aead_decrypt failed, algorithm not supported");
                Err(ResponseStatus::PsaErrorNotSupported)
            }
        }
    }
}
