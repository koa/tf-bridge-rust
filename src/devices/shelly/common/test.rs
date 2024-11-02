use crate::devices::shelly::light;
use crate::devices::shelly::shelly::SwitchingKey;
use crate::serde::SerdeStringKey;
use std::str::FromStr;

#[test]
fn test_parse_actor_key() {
    let actor_key = SwitchingKey::Light(SerdeStringKey(light::LightKey { id: 0 }));
    let result = actor_key.to_string();
    println!("Result: {actor_key}");
    assert_eq!(actor_key, SwitchingKey::from_str(result.as_str()).unwrap());
}
