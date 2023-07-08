use std::net::IpAddr;

use jsonwebtoken::{DecodingKey, EncodingKey};
use serde::{Deserialize, Serialize};

use crate::common::{utils::get_elapsed_milis, Id};

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
pub const OWNER_AUTH_COOKIE: &str = "owner_auth";
pub const OWNER_AUTH_CHECKED_COOKIE: &str = "checked_auth";

const SUB: &str = "sturdy@spoon.com";
const COMPANY: &str = "STURDY_SPOON";

#[derive(Serialize, Deserialize)]
pub struct OwnerAuth {
    pub username: String,
    pub room_id: Id,
    pub ip_addr: IpAddr,
    pub user_agent: String,
    sub: String,
    company: String,
    pub exp: u128,
}

impl OwnerAuth {
    pub fn new(
        username: String,
        room_id: Id,
        ip_addr: IpAddr,
        user_agent: String,
        exp: u128,
    ) -> Self {
        Self {
            username,
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

    #[inline]
    pub fn encode(&self, keys: &Keys) -> String {
        jsonwebtoken::encode(&jsonwebtoken::Header::default(), &self, &keys.encoding)
            .expect("Shouldn't fail")
    }

    #[inline]
    pub fn is_valid(&self, addr: IpAddr, user_agent: &String) -> bool {
        if get_elapsed_milis() > self.exp {
            return false;
        }
        self.ip_addr == addr && self.user_agent == *user_agent
    }

    #[inline]
    pub fn is_valid_room_id(&self, addr: IpAddr, user_agent: &String, room_id: &Id) -> bool {
        self.is_valid(addr, user_agent) && self.room_id == *room_id
    }
}
