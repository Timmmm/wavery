
function encode_length(input: bv64) : nat
{
    if input < (1 << 7) then 1 else 1 + encode_length(input >> 7)
}

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
