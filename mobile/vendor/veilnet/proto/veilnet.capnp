@0xb4311fa2593bcb51;

# A 256-bit value.
struct Key256 @0x80e473a80ea660a7 {
  p0 @0 :UInt64;
  p1 @1 :UInt64;
  p2 @2 :UInt64;
  p3 @3 :UInt64;
}

# A 256-bit public key, used for encryption and signing.
using PublicKey = Key256;

# A 256-bit record key into a distributed hash table (DHT).
using RecordKey = Key256;

# A 4-byte cryptosystem identifier.
using CryptoKind = UInt32;

# A 256-bit record key, identified within a specific cryptosystem.
struct TypedRecordKey @0x9dca32e8629f6a06 {
  kind @0 :CryptoKind;
  key @1 :RecordKey;
  secret @2 :Data;
}

# A 256-bit public key within a specific cryptosystem.
struct TypedPublicKey @0xa46975506779408c {
  kind @0 :CryptoKind;
  key @1 :PublicKey;
}

# A network route, published to a DHT subkey.
struct DhtRoute @0xa7d5ffa7dffbaaaa {
  # Route data used to import the private route, in order to communicate with it.
  routeData @0 :Data;

  # Owner public key used to certify communications belong to this route.
  ownerKey @1 :TypedPublicKey;
}

# A datagram intended for the route recipient.
struct Datagram @0xe236b5c319d5d4c7 {
  # The source address of the packet, which identifies the DHT key of the
  # sender's route.
  sourceAddr @0 :TypedRecordKey;

  # The source port of the packet, which identifies the DHT subkey of the
  # sender's route.
  sourcePort @1 :UInt16;

  # Signer of the datagram, must match the DHTRoute.ownerKey claimed at sourceAddr.
  ownerKey @2 :TypedPublicKey;

  # The contents of the packet, an opaque payload.
  contents @3 :Data;

  # Sequence number (optional), used for organizing datagram segments into a
  # stream.
  sequence @4 :UInt32 = 0;
}

# A signed packet containing a datagram.
struct Packet @0xde0038cc83a71037 {
  # A serialized datagram
  datagram @0 :Data;

  # Signature of the serialized datagram, made by ownerKey.
  signature @1 :Data;
}
