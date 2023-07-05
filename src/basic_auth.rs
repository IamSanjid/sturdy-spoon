use std::net::IpAddr;

use jsonwebtoken::{DecodingKey, EncodingKey};
use serde::{Deserialize, Serialize};

use crate::common::Id;

pub struct Keys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}

impl Keys {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

pub const EXPIRATION: u128 = 2 * 3600 * 1000; // 2 hours

const SUB: &str = "sturdy@spoon.com";
const COMPANY: &str = "STURDY_SPOON";

#[derive(Serialize, Deserialize)]
pub struct OwnerAuth {
    pub room_id: Id,
    pub ip_addr: IpAddr,
    pub user_agent: String,
    sub: String,
    company: String,
    pub exp: u128,
}

impl OwnerAuth {
    pub fn new(room_id: Id, ip_addr: IpAddr, user_agent: String, exp: u128) -> Self {
        Self {
            room_id,
            ip_addr,
            user_agent,
            sub: SUB.into(),
            company: COMPANY.into(),
            exp,
        }
    }

    pub fn from_token<S: AsRef<str>>(
        token: S,
        keys: &Keys,
    ) -> Result<Self, jsonwebtoken::errors::Error> {
        let auth = jsonwebtoken::decode::<OwnerAuth>(
            token.as_ref(),
            &keys.decoding,
            &jsonwebtoken::Validation::default(),
        )?;
        Ok(auth.claims)
    }

    pub fn encode(&self, keys: &Keys) -> String {
        jsonwebtoken::encode(&jsonwebtoken::Header::default(), &self, &keys.encoding)
            .expect("Shouldn't fail")
    }
}
