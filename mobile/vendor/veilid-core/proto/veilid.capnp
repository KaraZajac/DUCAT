@0xf981aec14891e605;

# Typed IDs and Hashes
##############################

# DHT Record Key
struct OpaqueRecordKey @0x875582886b9407f2 {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# Blockstore Block Id
struct BlockId @0xebd7e6cd09f6898c {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# Node Id (hash of node public key)
struct NodeId @0x8f7d5499988a6677 {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# Public Key
struct PublicKey @0x9308e9f7a544f277 {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# DHT Route Id
struct RouteId @0xefbdc842e2f736ff {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# Signature
struct Signature @0xaa5512a00eece048 {
    kind                    @0  :CryptoKind;
    value                   @1  :Data;
}

# Untyped generic one-time encryption nonce
struct Nonce @0xb1eebc5d39502e45 {
    value                   @0  :Data;
}


# Convenience Typedefs
################################################################

using TunnelId = UInt64;                                # Id for tunnels
using CryptoKind = UInt32;                              # FOURCC code for cryptography type
using ValueSeqNum = UInt32;                             # sequence numbers for values
using Subkey = UInt32;                                  # subkey index for dht
using Capability = UInt32;                              # FOURCC code for capability
using ProtocolType = UInt32;                            # FOURCC code for protocol type
using AddressType = UInt32;                             # FOURCC code for addresss type
using DialInfoClass = UInt32;                           # FOURCC code for dial info class
using Sequencing = UInt32;                              # FOURCC code for sequencing requirement
using RelayKind = UInt32;                               # FOURCC code for relay kind
using EnvelopeVersion = UInt32;                         # FOURCC code for envelope version

# Node Dial Info
################################################################

struct AddressIPV4 @0xc3e871e66c7dadd9 {
    addr                    @0  :UInt32;                # Address in big endian format
}

struct AddressIPV6 @0xde8c1e2547115847 {
    addr0                   @0  :UInt32;                # \ 
    addr1                   @1  :UInt32;                #  \ Address in big 
    addr2                   @2  :UInt32;                #  / endian format
    addr3                   @3  :UInt32;                # / 
}

struct Address @0x8b6c454a8571fd72 {
    addressType             @0  :AddressType;           # The fourcc code for this address type
    detail                  @1  :AnyPointer;            # The struct corresponding to this AddressType (DialInfoUDP/TCP/WS/etc)
}

struct SocketAddress @0xa65d3b808f51cc29 {
    address                 @0  :Address;
    port                    @1  :UInt16;
}

struct DialInfoUDP @0xd3a8814bc946a191 {
    socketAddress           @0  :SocketAddress;
}

struct DialInfoTCP @0xf5214996436db82b {
    socketAddress           @0  :SocketAddress;
}

struct DialInfoWS @0xc76541dea44417c3 {
    socketAddress           @0  :SocketAddress;
    request                 @1  :Text;
}

struct DialInfoWSS @0xe639faa41b7d7b04 {
    socketAddress           @0  :SocketAddress;
    request                 @1  :Text;
}

struct DialInfo @0xfd40924f8b57d9a5 {
    protocolType            @0  :ProtocolType;          # Type of protocol this dialinfo is for
    detail                  @1  :AnyPointer;            # The struct corresponding to this protocoltype (DialInfoUDP/TCP/WS/etc)
}

# Signals
##############################

struct SignalInfoHolePunch @0xaa24841378d6dabf {
    receipt                 @0  :Data;                  # receipt to return with hole punch
    peerInfo                @1  :PeerInfo;              # peer info of the signal sender for hole punch attempt
    dialInfo                @2  :DialInfo;              # (optional) dial info to use, otherwise rawudp dialinfo from the peerinfo is assumed 
}

struct SignalInfoReverseConnect @0x9be3baab08513db2 {
    receipt                 @0  :Data;                  # receipt to return with reverse connect
    peerInfo                @1  :PeerInfo;              # peer info of the signal sender for reverse connect attempt
}

# Private Routes
##############################

struct RouteHopData @0xe7db537b31c44117 {
    nonce                   @0  :Nonce;                 # nonce for encrypted blob
    blob                    @1  :Data;                  # encrypted blob with ENC(nonce,DH(PK,SK))
                                                        # if this is a safety route RouteHopData, there is a single byte tag appended to the end of the encrypted blob
                                                        # it can be one of: 
                                                        #     if more hops remain in this route: RouteHop (0 byte appended as tag)
                                                        #     if end of safety route and starting private route: PrivateRoute (1 byte appended as tag)
                                                        # if this is a private route RouteHopData, only can decode to RouteHop, no tag is appended
}

struct RouteHop @0xf09cfdc51e7bf771 {
    node :union {                                       
        nodeId              @0  :NodeId;                # node id key only for established routes (kind is the same as the pr or sr it is part of)
        peerInfo            @1  :PeerInfo;              # full peer info for this hop to establish the route
    }
    nextHop                 @2  :RouteHopData;          # optional: If this the end of a private route, this field will not exist
                                                        # if this is a safety route routehop, this field is not optional and must exist
}

struct PrivateRoute @0xd6bd71c43df5a4f4 {
    publicKey               @0  :PublicKey;             # private route public key (unique per private route)
    hops :union {
        firstHop            @1  :RouteHop;              # first hop of a private route is unencrypted (hopcount > 0)
        data                @2  :RouteHopData;          # private route has more hops (hopcount > 0 && hopcount < total_hopcount)
        empty               @3  :Void;                  # private route has ended (hopcount = 0)
    }   
} 

struct SafetyRoute @0xf1aea230a7add37a {
    publicKey               @0  :PublicKey;             # safety route public key (unique per safety route)
    hops :union {
        data                @1  :RouteHopData;          # safety route has more hops
        private             @2  :PrivateRoute;          # safety route has ended and private route follows
    }
}

# Operations
##############################

struct DialInfoDetail @0xef806eed2f347d1a {
    dialInfo                @0  :DialInfo;
    class                   @1  :DialInfoClass;
}

struct NodeStatus @0xce93d3a69815c3fe {
    # Reserved for non-nodeinfo status
}

struct SenderInfo @0xbad425a899f50b53 {
    socketAddress           @0  :SocketAddress;         # socket address that for the sending peer
}

struct CryptoInfo @0xc7fe5dd7f7901475 {
    cryptoKind              @0  :CryptoKind;            # Type of cryptography this info is for
    detail                  @1  :AnyPointer;            # The struct corresponding to this crypto kind (CryptoInfoVLD0/1/etc)
}

struct CryptoInfoNONE @0x893df8e1673e9ed5 {
    publicKey               @0  :Data;                  # Combination node-id, key agreement dh key, and public signing key
}

struct CryptoInfoVLD0 @0xc71d82d73800a0a8 {
    publicKey               @0  :Data;                  # Combination node-id, key agreement dh key, and public signing key
}

struct CryptoInfoVLD1 @0xc0848d50fdbf004a {
    encapsulationKey        @0  :Data;                  # Current KEM encapsulation key (used by: VLD1/ML-KEM: 1184 bytes)
    signingKey              @1  :Data;                  # Signing Key (use by: VLD1/ML-DSA: 1952 bytes)
}

# Just enough information to describe a single relay, for the RLAY capability
# Relays must support the same set of envelopes as the node being relayed for
# Relays support whatever cryptography is used on their node ids
# Relays have no crypto info because they don't decrypt or validate envelopes
# They may have different address types supported, and only a subset of the dialinfo of the relay node's full peerinfo
# `outboundProtocols` and `addressTypes` are only use for outbound-relaying, which is not enabled
# for any Veilid node configuration today.
# RelayInfo is not signed by the relay, and as such should not be added to the routing table
# RelayInfo has nodeIds solely for the purpose of communicating to the relay a commitment to using that relay for this node
struct RelayInfo @0xe5814c9bf77c5d64 {
    timestamp               @0  :UInt64;                # the timestamp of when the relay info was generated (us since epoch)
    nodeIds                 @1  :List(NodeId);          # node ids for relay
    outboundProtocols       @2  :List(ProtocolType);    # protocols that can go outbound
    addressTypes            @3  :List(AddressType);     # address types supported
    dialInfoDetailList      @4  :List(DialInfoDetail);  # inbound dial info details for this node
    relayKind               @5  :RelayKind;                  # type of relay
}

# Full node information to be signed into a peerinfo
struct NodeInfo @0xaab6a61cc9dc8e34 {
    timestamp               @0  :UInt64;                # when node info was generated (us since epoch)
    envelopeSupport         @1  :List(EnvelopeVersion); # supported rpc envelope versions (receipt versions follow envelope versions)
    cryptoInfoList          @2  :List(CryptoInfo);      # cryptography info per supported crypto kind
    capabilities            @3  :List(Capability);      # capabilities supported by the node
    outboundProtocols       @4  :List(ProtocolType);    # protocols that can go outbound
    addressTypes            @5  :List(AddressType);     # address types supported
    dialInfoDetailList      @6  :List(DialInfoDetail);  # inbound dial info details for this node
    relayInfoList           @7  :List(RelayInfo);       # relay node info, just enough to get connected
}

struct PeerInfo @0xca2ea684ebbdad2f {
    nodeInfoMessage         @0  :Data;                  # capnp message of NodeInfo being signed
    signatures              @1  :List(Signature);       # signatures over the nodeInfoMessage for each CryptoKind, via the public keys in the nodeInfoMessage (VLD0: 64 bytes, VLD1: 3309 bytes)
}

struct RoutedOperation @0xc8e9c493627bb3f2 {
    sequencing              @0  :Sequencing;            # sequencing preference to use to pass the message along
    signatures              @1  :List(Signature);       # signatures from nodes that have handled the private route
    nonce                   @2  :Nonce;                 # nonce Xmsg
    data                    @3  :Data;                  # operation message encrypted with ENC(Xmsg,DH(PKapr,SKbsr))
}

struct OperationStatusQ @0x865d80cea70d884a {
    nodeStatus              @0  :NodeStatus;            # optional: node status update about the statusq sender
}

struct OperationStatusA @0x841a6fb922d2d61a {
    nodeStatus              @0  :NodeStatus;            # optional: returned node status
    senderInfo              @1  :SenderInfo;            # optional: info about StatusQ sender from the perspective of the replier
}

struct OperationValidateDialInfo @0xe4e14b9be1cee00d {
    dialInfo                @0  :DialInfo;              # dial info to use for the receipt
    receipt                 @1  :Data;                  # receipt to return to dial info to prove it is reachable
    redirect                @2  :Bool;                  # request a different node do the validate
}

struct OperationReturnReceipt @0x9403031d5341761c {
    receipt                 @0  :Data;                  # receipt being returned to its origin
}

struct OperationFindNodeQ @0xb1dee545e359e779 {
    nodeId                  @0  :NodeId;                # node id to locate
    capabilities            @1  :List(Capability);      # required capabilities returned peers must have
}

struct OperationFindNodeA @0xc033b926302deb1a {
    peers                   @0  :List(PeerInfo);        # returned 'closer peer' information
}

struct OperationRoute @0xebddc421ed8854b7 {
    safetyRoute             @0  :SafetyRoute;           # where this should go
    operation               @1  :RoutedOperation;       # the operation to be routed
}

struct OperationAppCallQ @0xcf68e044fb4937bf {
    message                 @0  :Data;                  # opaque request to application
}

struct OperationAppCallA @0xc47f7d3ebc5611e2 {
    message                 @0  :Data;                  # opaque response from application
}

struct OperationAppMessage @0xa235c5febdd85c98 {
    message                 @0  :Data;                  # opaque message to application
}

struct SubkeyRange @0xeda3078ac0f1ec6b {
    start                   @0  :Subkey;                # the start of a subkey range
    end                     @1  :Subkey;                # the end of a subkey range
}

struct ValueData @0xacbe86e97ace772a {
    seq                     @0  :ValueSeqNum;           # sequence number of value
    data                    @1  :Data;                  # subkey contents
    writer                  @2  :PublicKey;             # the public key of the writer
    nonce                   @3  :Nonce;                 # nonce used for `data` encryption
}

struct SignedValueData @0xbc21055c2442405f {
    valueData               @0  :Data;                  # ValueData serialized to bytes
    signature               @1  :Signature;             # signature of data at this subkey, using the writer key (which may be the same as the owner key)
                                                        # signature covers:
                                                        #  * owner public key
                                                        #  * subkey
                                                        #  * sequence number
                                                        #  * data
                                                        #  * nonce
                                                        # signature does not need to cover schema because schema is validated upon every set
                                                        # so the data either fits, or it doesn't.
}

struct SignedValueDescriptor @0xf6ffa63ef36d0f73 {
    owner                   @0  :PublicKey;             # the public key of the owner
    schemaData              @1  :Data;                  # the schema data
                                                        # Changing this after key creation is not supported as it would change the dht key
    signature               @2  :Signature;             # Schema data is signed by ownerKey and is verified both by set and get operations
}


struct OperationGetValueQ @0x83b34ce1e72afc7f {
    key                     @0  :OpaqueRecordKey;       # DHT Key = Hash(ownerKeyKind) of: [ ownerKeyValue, schema ]
    subkey                  @1  :Subkey;                # the index of the subkey
    wantDescriptor          @2  :Bool;                  # whether or not to include the descriptor for the key
}


struct OperationGetValueA @0xf97edb86a914d093 {
    accepted                @0  :Bool;                  # true if the operation was accepted by the distance metric
    value                   @1  :SignedValueData;       # optional: the value if successful, or if unset, no value returned
    peers                   @2  :List(PeerInfo);        # returned 'closer peer' information on either success or failure
    descriptor              @3  :SignedValueDescriptor; # optional: the descriptor if requested if the value is also returned
}

struct OperationSetValueQ @0xb315a71cd3f555b3 {
    key                     @0  :OpaqueRecordKey;       # DHT Key = Hash(ownerKeyKind) of: [ ownerKeyValue, schema ]
    subkey                  @1  :Subkey;                # the index of the subkey
    value                   @2  :SignedValueData;       # subkey contents (older or equal seq number gets dropped)
    descriptor              @3  :SignedValueDescriptor; # optional: the descriptor if needed
}

struct OperationSetValueA @0xb5ff5b18c0d7b918 {
    accepted                @0  :Bool;                  # true if the operation was accepted by the distance metric
    needDescriptor          @1  :Bool;                  # true if the descriptor was not sent but it was needed
    value                   @2  :SignedValueData;       # optional: the current value at the key if the set seq number was lower or equal to what was there before
    peers                   @3  :List(PeerInfo);        # returned 'closer peer' information on either success or failure
}

struct OperationWatchValueQ @0xddae6e08cea11e84 {
    key                     @0  :OpaqueRecordKey;       # key for value to watch
    subkeys                 @1  :List(SubkeyRange);     # subkey range to watch (up to 512 subranges). An empty range here should not be specified unless cancelling a watch (count=0).
    duration                @2  :UInt64;                # requested duration when this watch will expire in usec since creation (watch can return less, 0 for max)
    count                   @3  :UInt32;                # requested number of changes to watch for (0 = cancel, 1 = single shot, 2+ = counter, UINT32_MAX = continuous)
    watchId                 @4  :UInt64;                # if 0, request a new watch. if >0, existing watch id 
}

struct OperationWatchValueA @0xaeed4433b1c35108 {
    accepted                @0  :Bool;                  # true if the operation was accepted by the distance metric
    duration                @1  :UInt64;                # usecs remaining until this watch will expire (0 if watch was cancelled/dropped)
    peers                   @2  :List(PeerInfo);        # returned list of other nodes to ask that could propagate watches
    watchId                 @3  :UInt64;                # (0 = id not allocated if rejecting new watch) random id for watch instance on this node
}

struct OperationInspectValueQ @0xe4d014b5a2f6ffaf {
    key                     @0  :OpaqueRecordKey;       # DHT Key = Hash(ownerKeyKind) of: [ ownerKeyValue, schema ]
    subkeys                 @1  :List(SubkeyRange);     # subkey range to inspect (up to 1024 total subkeys), if empty this implies 0..=511
    wantDescriptor          @2  :Bool;                  # whether or not to include the descriptor for the key
}

struct OperationInspectValueA @0x8540edb633391b2a {
    accepted                @0  :Bool;                  # true if the operation was accepted by the distance metric
    seqs                    @1  :List(ValueSeqNum);     # the list of subkey value sequence numbers in ascending order for each subkey in the requested range. if a subkey has not been written to, it is given a value of UINT32_MAX. these are not signed, and may be immediately out of date, and must be verified by a GetValueQ request.
    peers                   @2  :List(PeerInfo);        # returned 'closer peer' information on either success or failure
    descriptor              @3  :SignedValueDescriptor; # optional: the descriptor if requested if the value is also returned
}

struct OperationValueChanged @0xbf9d00e88fd96623 {
    key                     @0  :OpaqueRecordKey;       # key for value that changed
    subkeys                 @1  :List(SubkeyRange);     # subkey range that changed (up to 512 ranges at a time, if empty this is a watch expiration notice)
    count                   @2  :UInt32;                # remaining changes left (0 means watch has expired)
    watchId                 @3  :UInt64;                # watch id this value change came from
    value                   @4  :SignedValueData;       # Optional: first value that changed, if it was the only change in its transaction. (the rest can be gotten with getvalue)
}

enum TransactCommand @0xa841a757a9a7f946 {
    end                     @0;                         # end the transaction (called after start to prepare to commit)
    commit                  @1;                         # commit all operations (called after end)
    rollback                @2;                         # roll back all operations (called at any time after start)
    get                     @3;                         # get a subkey value from a transaction
    set                     @4;                         # set a subkey value in a transaction
    # sync                  @5;                         # (placeholder for sync operation in 0.8.0)
}

struct OperationTransactBeginQ @0xf8629eff87ac729d {
    key                     @0  :OpaqueRecordKey;       # key for record to transact on
    descriptor              @1  :SignedValueDescriptor; # optional: the descriptor if needed
    wantDescriptor          @2  :Bool;                  # whether or not to include the descriptor for the key
}

struct OperationTransactBeginA @0xd2b5a46f55268aa4 {
    accepted                @0  :Bool;                  # true if the operation was accepted by the distance metric
    needDescriptor          @1  :Bool;                  # true if the descriptor was not sent but it was needed
    descriptor              @2  :SignedValueDescriptor; # optional: the descriptor if wanted
    transactionId           @3  :UInt64;                # transaction id if successful, 0 if operation failed
    duration                @4  :UInt64;                # expiration duration, 0 if operation failed
    seqs                    @5  :List(ValueSeqNum);     # optional: the list of subkey value sequence numbers in ascending order for each subkey
    peers                   @6  :List(PeerInfo);        # optional: returned 'closer peer' information on either success or failure
}

struct OperationTransactCommandQ @0xb33bdbef4d26ba3f {
    key                     @0  :OpaqueRecordKey;       # key for record to transact on
    transactionId           @1  :UInt64;                # transaction id
    command                 @2  :TransactCommand;       # command to execute
    seqs                    @3  :List(ValueSeqNum);     # optional sync: the list of subkey value sequence numbers in ascending order for each subkey
    subkey                  @4  :Subkey;                # get/set, optional sync: the index of the subkey
    value                   @5  :SignedValueData;       # set, optional sync: subkey contents
}

struct OperationTransactCommandA @0xe68ff85399f3622c {
    transactionValid        @0  :Bool;                  # true if the transaction id was valid and the operation succeeded
    duration                @1  :UInt64;                # updated expiration duration, 0 if operation failed or the transaction ended
    seqs                    @2  :List(ValueSeqNum);     # sync: the list of subkey value sequence numbers in ascending order for each subkey
    subkey                  @3  :Subkey;                # optional sync: the index of the subkey
    value                   @4  :SignedValueData;       # optional get/set/sync: subkey contents
}

struct OperationSupplyBlockQ @0xe0d00fd8091dd2e0 {
    blockId                 @0  :BlockId;               # hash of the block we can supply
    routeId                 @1  :RouteId;               # the private route endpoint for this block supplier
}

struct OperationSupplyBlockA @0xc7421cd5b08b8abe {
    duration                @0  :UInt64;                # usecs until the block supplier entry will need to be refreshed, or 0 if not successful
    peers                   @1  :List(PeerInfo);        # returned 'closer peer' information if not successful       
}

struct OperationFindBlockQ @0xbda4ed3b68c636d3 {
    blockId                 @0  :BlockId;               # hash of the block to locate
}

struct OperationFindBlockA @0xfea1afb737b1a2a5 {
    data                    @0  :Data;                  # Optional: the actual block data if we have that block ourselves
                                                        # null if we don't have a block to return
    suppliers               @1  :List(RouteId);         # returned list of supplier private route ids if we have them
    peers                   @2  :List(PeerInfo);        # returned 'closer peer' information 
}

struct OperationSignal @0xf751cb24dd510a4e {
    union {
        holePunch           @0  :SignalInfoHolePunch;
        reverseConnect      @1  :SignalInfoReverseConnect;
    }
}

enum TunnelEndpointMode @0x8da1d6126622670e {
    raw                     @0;                         # raw tunnel
    turn                    @1;                         # turn tunnel
}

enum TunnelError @0x93fd4ac3ba42bad6 {
    badId                   @0;                         # Tunnel ID was rejected
    noEndpoint              @1;                         # Endpoint was unreachable
    rejectedMode            @2;                         # Endpoint couldn't provide mode
    noCapacity              @3;                         # Endpoint is full
}

struct TunnelEndpoint @0xae60fa94d9003ecf {
    mode                    @0  :TunnelEndpointMode;    # what kind of endpoint this is
    description             @1  :Data;                  # endpoint description (TODO)
}

struct FullTunnel @0xba75346760f8ca96 {
    id                      @0  :TunnelId;              # tunnel id to use everywhere
    duration                @1  :UInt64;                # usecs from last data when this expires if no data is sent or received
    local                   @2  :TunnelEndpoint;        # local endpoint
    remote                  @3  :TunnelEndpoint;        # remote endpoint
}

struct PartialTunnel @0xfbe76bb5cd30ff89 {
    id                      @0  :TunnelId;              # tunnel id to use everywhere
    duration                @1  :UInt64;                # usecs until this expires if not completed
    local                   @2  :TunnelEndpoint;        # local endpoint
}

struct OperationStartTunnelQ @0xbaaeb21d66a3eb38 {
    id                      @0  :TunnelId;              # tunnel id to use everywhere
    localMode               @1  :TunnelEndpointMode;    # what kind of local endpoint mode is being requested
    depth                   @2  :UInt8;                 # the number of nodes in the tunnel
}

struct OperationStartTunnelA @0xccae2bb59891fccd {
    union {
        partial             @0  :PartialTunnel;         # the first half of the tunnel
        error               @1  :TunnelError;           # if we didn't start the tunnel, why not
    }
}

struct OperationCompleteTunnelQ @0x81b8e303b58f83dd {
    id                      @0  :TunnelId;              # tunnel id to use everywhere
    localMode               @1  :TunnelEndpointMode;    # what kind of local endpoint mode is being requested
    depth                   @2  :UInt8;                 # the number of nodes in the tunnel
    endpoint                @3  :TunnelEndpoint;        # the remote endpoint to complete
}

struct OperationCompleteTunnelA @0xe6c0670bb6d0d0fb {
    union {
        tunnel              @0  :FullTunnel;            # the tunnel description
        error               @1  :TunnelError;           # if we didn't complete the tunnel, why not
    }
}

struct OperationCancelTunnelQ @0xec9f521d84a28961 {
    id                      @0  :TunnelId;              # the tunnel id to cancel
}

struct OperationCancelTunnelA @0x920079ef493c9294 {
    union {
        tunnel              @0  :TunnelId;              # the tunnel id that was cancelled
        error               @1  :TunnelError;           # if we couldn't cancel, why not
    }
}

# Things that want an answer
struct Question @0xcb35ddc42056db29 {
    respondTo :union {
        sender              @0  :Void;                  # sender
        privateRoute        @1  :PrivateRoute;          # embedded private route to be used for reply
    }
    detail :union {
        # Direct operations
        statusQ             @2  :OperationStatusQ;
        findNodeQ           @3  :OperationFindNodeQ;
        
        # Routable operations
        appCallQ            @4  :OperationAppCallQ;
        getValueQ           @5  :OperationGetValueQ;
        setValueQ           @6  :OperationSetValueQ;
        watchValueQ         @7  :OperationWatchValueQ;
        inspectValueQ       @8  :OperationInspectValueQ;
        transactBeginQ      @9  :OperationTransactBeginQ;
        transactCommandQ    @10  :OperationTransactCommandQ;

        # Blockstore operations
        # #[cfg(feature="unstable-blockstore")]
        # supplyBlockQ        @11  :OperationSupplyBlockQ;
        # findBlockQ          @12  :OperationFindBlockQ;
        
        # Tunnel operations
        # #[cfg(feature="unstable-tunnels")]
        # startTunnelQ        @13 :OperationStartTunnelQ;
        # completeTunnelQ     @14 :OperationCompleteTunnelQ;
        # cancelTunnelQ       @15 :OperationCancelTunnelQ;
    }
}

# Things that don't want an answer
struct Statement @0xca0ff5973692c050 {
    detail :union {
        # Direct operations
        validateDialInfo    @0  :OperationValidateDialInfo;
        route               @1  :OperationRoute;
        
        # Routable operations
        signal              @2  :OperationSignal;
        returnReceipt       @3  :OperationReturnReceipt;
        appMessage          @4  :OperationAppMessage;
        valueChanged        @5  :OperationValueChanged;
    }
}

# Things that are answers
struct Answer @0x8edae77299061a3b {
    detail :union {
        # Direct operations
        statusA             @0  :OperationStatusA;
        findNodeA           @1  :OperationFindNodeA;
        
        # Routable operations
        appCallA            @2  :OperationAppCallA;
        getValueA           @3  :OperationGetValueA;
        setValueA           @4  :OperationSetValueA;
        watchValueA         @5  :OperationWatchValueA;
        inspectValueA       @6  :OperationInspectValueA;
        transactBeginA      @7  :OperationTransactBeginA;
        transactCommandA    @8  :OperationTransactCommandA;

        # Blockstore operations
        # #[cfg(feature="unstable-blockstore")]
        # supplyBlockA        @9  :OperationSupplyBlockA;
        # findBlockA          @10  :OperationFindBlockA;
    
        # Tunnel operations
        # #[cfg(feature="unstable-tunnels")]
        # startTunnelA        @11  :OperationStartTunnelA;
        # completeTunnelA     @12  :OperationCompleteTunnelA;
        # cancelTunnelA       @13  :OperationCancelTunnelA;
    }
}

struct Operation @0x93ff8a43a01d6f5a {
    opId                    @0  :UInt64;                # Random RPC ID. Must be random to foil reply forgery attacks. 
    senderPeerInfo          @1  :PeerInfo;              # optional: PeerInfo for the sender to be cached by the receiver.
    targetNodeInfoTs        @2  :UInt64;                # Timestamp the sender believes the target's node info to be at or zero if not sent
    kind :union {
        question            @3  :Question;
        statement           @4  :Statement;
        answer              @5  :Answer;
    }
}

struct SignedOperation @0xbf5bbfe20d95c293 {
    operationData           @0  :Data;                  # the operation rpc in serialized capnp format
    signer                  @1  :PublicKey;             # optional: the signer performing the operation. common signers include:
                                                        #  * owner or a schema member (used by watchValue, transactBegin)
                                                        #  * generated anonymous key (used by watchValue)
                                                        #  * a node public key of a nearby node (used by transactBegin)
    signature               @2  :Signature;             # optional: required if signer is specified. Signature of the signer. Signature covers:
                                                        #  * operationData blob
                                                        #  * destinationKey:
                                                        #    - Node public key
                                                        #    - PrivateRoute public key
}

