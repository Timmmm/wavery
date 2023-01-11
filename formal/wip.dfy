// newtype {:nativeType "u8"} u8 = x | 0 <= x < 256
// newtype {:nativeType "u64"} u64 = x | 0 <= x < 18446744073709551616

function decode_varint(input: seq<bv8>) : bv64
    requires |input| > 0
{
    var byte := input[0];
    var val := (byte & 0x7F) as bv64;
    var more := byte & 0x80 == 0 && |input| > 1;

    if more then val | (decode_varint(input[1..]) << 7) else val
}

function encode_varint(input: bv64) : seq<bv8>
{
    var byte := (input & 0x7F) as bv8;
    var shifted := input >> 7;
    if shifted == 0 then [byte | 0x80] else [byte] + encode_varint(shifted)
}

function encode_length(input: bv64) : nat
{
    if input < (1 << 7) then 1 else 1 + encode_length(input >> 7)
}

lemma EncodeLengthIsCorrect(input: bv64)
    ensures |encode_varint(input)| == encode_length(input)
{}

lemma LastByteTagged(input: bv64)
    ensures var e := encode_varint(input); e[|e|-1] & 0x80 != 0
{}

lemma EarlyBytesNotTagged(input: bv64)
    ensures var e := encode_varint(input); forall i :: 0 <= i < |e|-1 ==> e[i] & 0x80 == 0
{}

lemma EncodeLengthForValueRange(input: bv64, length: nat)
    requires 1 <= length <= 10
    requires (1 << ((length - 1) * 7)) <= input
    requires length < 10 ==> input < (1 << (length * 7))
    ensures encode_length(input) == length
{
    if length > 1 {
        EncodeLengthForValueRange(input >> 7, length - 1);
    }
}

lemma EncodeLengthMonotonic(input: bv64)
    ensures input < 0xFFFFFFFFFFFFFFFF ==> encode_length(input + 1) >= encode_length(input)
{}

// lemma EncodeLength(input: bv64)
//     ensures |encode_varint(input)| <= 10
// {
//     assert forall i :: (1 << 9*7) <= i ==> EncodeLengthForValueRange
//     EncodeLengthForValueRange(0xFFFFFFFFFFFFFFFF, 10);
// }

lemma EncodeLengthDecrease(input: bv64)
    decreases input
    ensures encode_length(input) == if input < (1 << 7) then 1 else encode_length(input >> 7) + 1 // This is the same as the encode_length function...
{
    if input > 0 {
        EncodeLengthDecrease(input >> 7);
    }
}

lemma EncodeLength1(input: bv64)
    ensures input < (1 << 7) ==> encode_length(input) <= 1
{}

lemma EncodeLength2(input: bv64)
    ensures input < (1 << 14) ==> encode_length(input) <= 2
{}

lemma EncodeLength3(input: bv64)
    ensures input < (1 << 21) ==> encode_length(input) <= 3
{
    EncodeLengthDecrease(input >> 7);
}

lemma EncodeLength4(input: bv64)
    ensures input < (1 << 28) ==> encode_length(input) <= 4
{
    EncodeLengthDecrease(input >> 7);
}

lemma EncodeLength5(input: bv64)
    ensures input < (1 << 35) ==> encode_length(input) <= 5
{
    EncodeLengthDecrease(input >> 7);
}

lemma EncodeLength6(input: bv64)
    ensures input < (1 << 42) ==> encode_length(input) <= 6
{
    EncodeLengthDecrease(input >> 7);
}

lemma EncodeLength7(input: bv64)
    ensures input < (1 << 49) ==> encode_length(input) <= 7
{
    if (1 << 28 <= input) {
        EncodeLengthForValueRange(input, 5);
    }
}
// // If it's 10 bytes there can only be one value for the last byte. Hmm this actually
// // means it's redundant so we could always encode into 9 bytes. Interesting.
// lemma LastByteFullLength(input: bv64)
//     ensures var e := encode_varint(input); |e| == 10 ==> e[9] == 0x81
// {}

// lemma ByteContent(input: bv64)
//     ensures var e := encode_varint(input); forall i :: 0 <= i < |e| ==> (e[i] & 0x7F) == ((input >> 7*i) & 0x7F) as bv8
// {}

// Dafny automatically performs induction on lemmas, but not asserts.
lemma Lossless(input: bv64)
    ensures decode_varint(encode_varint(input)) == input
{}

// This is not strictly necessary but good for sanity.
lemma LosslessAssertion(input: bv64) {
    var encoded := encode_varint(input);
    var decoded := decode_varint(encoded);
    Lossless(input);
    assert decoded == input;
}

lemma NeverEmpty(input: bv64) {
    var encoded := encode_varint(input);
    assert |encoded| > 0;
}

// /// Decode an unsigned varint.
// method DecodeVarint(input: array<bv8>) returns (value: bv64)
//     requires input.Length >= 10;
//     ensures value == decode_varint(input[..]);
// {
//     var shift := 0;
//     var i := 0;
//     while (shift < 64)
//         decreases input.Length - i;
//         decreases 64 - shift;
//         invariant shift == i * 7;
//         invariant 0 <= i <= input.Length;
//     {

//         value := value | (((input[i] & 0x7F) as bv64) << shift);

//         if input[i] & 0x80 == 0 {
//             break;
//         }

//         shift := shift + 7;
//         i := i + 1;
//     }
// }


// method EncodeVarint(input: bv64) returns (output: seq<bv8>)
//     ensures output == encode_varint(input);
// {
//     var value := input;
//     var buffer := new bv8[10];

//     ghost var answer := encode_varint(input);
//     LastByteTagged(input);
//     assert answer[|answer| - 1] & 0x80 != 0;

//     for i := 0 to 10
//         // invariant i <= |answer|;
//         // invariant forall k :: 0 <= k < i && k < |answer| ==> buffer[k] == answer[k];
//     {
//         var bits := (value & 0x7F) as bv8;
//         value := value >> 7;
//         var more := value != 0;
//         if more {
//             bits := bits | 0x80;
//         }

//         buffer[i] := bits;
//         assert !more <==> i == |answer| - 1;
//         if !more {
//             return buffer[..i];
//         }
//         // assert i < |answer|;
//     }
//     return buffer[..];
// }

    // for byte in input {
    //     // Check for overflow.
    //     // This allows the compiler to unroll the loop. I'm not sure it is
    //     // faster tbh.
    //     if shift >= 64 {
    //         return None;
    //     }
    //     // Note that we don't check for overflow in the 10th byte (of which
    //     // only one bit is used), but never mind.
    //     value |= ((byte & 0x7F) as u64) << shift;
    //     // Check if we're finished.
    //     if byte & 0x80 == 0 {
    //         return Some(value);
    //     }
    //     shift += 7;
    // }
    // None
