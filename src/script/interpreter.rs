use keys::{Public, Signature};
use hash::H256;
use transaction::Transaction;
use script::{script, Script, Num, VerificationFlags, Opcode, Error, Instruction};

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum SignatureHash {
	All = 1,
	None = 2,
	Single = 3,
	AnyoneCanPay = 0x80,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SignatureVersion {
	Base,
	WitnessV0,
}

pub trait SignatureChecker {
	fn check_signature(
		&self,
		script_signature: &[u8],
		public: &Public,
		script: &Script,
		version: SignatureVersion
	) -> bool;

	fn check_lock_time(&self, lock_time: Num) -> bool;

	fn check_sequence(&self, sequence: Num) -> bool;
}

pub struct NoopSignatureChecker;

impl SignatureChecker for NoopSignatureChecker {
	fn check_signature(&self, _: &[u8], _: &Public, _: &Script, _: SignatureVersion) -> bool {
		false
	}

	fn check_lock_time(&self, _: Num) -> bool {
		false
	}

	fn check_sequence(&self, _: Num) -> bool {
		false
	}
}

pub struct TransactionSignatureChecker {
	transaction: Transaction,
	i: u32,
	amount: i64,
}

impl TransactionSignatureChecker {
	fn verify_signature(&self, _signature: &[u8], _public: &Public, _hash: &H256) -> bool {
		unimplemented!();
	}
}

impl SignatureChecker for TransactionSignatureChecker {
	fn check_signature(
		&self,
		_script_signature: &[u8],
		_public: &Public,
		_script: &Script,
		_version: SignatureVersion
	) -> bool {
		unimplemented!();
	}

	fn check_lock_time(&self, _lock_time: Num) -> bool {
		unimplemented!();
	}

	fn check_sequence(&self, _sequence: Num) -> bool {
		unimplemented!();
	}

}

fn is_public_key(v: &[u8]) -> bool {
	match v.len() {
		33 if v[0] == 2 || v[0] == 3 => true,
		65 if v[0] == 4 => true,
		_ => false,
	}
}

/// A canonical signature exists of: <30> <total len> <02> <len R> <R> <02> <len S> <S> <hashtype>
/// Where R and S are not negative (their first byte has its highest bit not set), and not
/// excessively padded (do not start with a 0 byte, unless an otherwise negative number follows,
/// in which case a single 0 byte is necessary and even required).
///
/// See https://bitcointalk.org/index.php?topic=8392.msg127623#msg127623
///
/// This function is consensus-critical since BIP66.
fn is_valid_signature_encoding(sig: &[u8]) -> bool {
	// Format: 0x30 [total-length] 0x02 [R-length] [R] 0x02 [S-length] [S] [sighash]
	// * total-length: 1-byte length descriptor of everything that follows,
	//   excluding the sighash byte.
	// * R-length: 1-byte length descriptor of the R value that follows.
	// * R: arbitrary-length big-endian encoded R value. It must use the shortest
	//   possible encoding for a positive integers (which means no null bytes at
	//   the start, except a single one when the next byte has its highest bit set).
	// * S-length: 1-byte length descriptor of the S value that follows.
	// * S: arbitrary-length big-endian encoded S value. The same rules apply.
	// * sighash: 1-byte value indicating what data is hashed (not part of the DER
	//   signature)

	// Minimum and maximum size constraints
	if sig.len() < 9 || sig.len() > 73 {
		return false;
	}

	// A signature is of type 0x30 (compound)
	if sig[0] != 0x30 {
		return false;
	}

	// Make sure the length covers the entire signature.
	if sig[1] as usize != sig.len() - 3 {
		return false;
	}

	// Extract the length of the R element.
	let len_r = sig[3] as usize;

	// Make sure the length of the S element is still inside the signature.
	if len_r + 5 >= sig.len() {
		return false;
	}

	// Extract the length of the S element.
	let len_s = sig[len_r + 5] as usize;

	// Verify that the length of the signature matches the sum of the length
	if len_r + len_s + 7 != sig.len() {
		return false;
	}

	// Check whether the R element is an integer.
	if sig[2] != 2 {
		return false;
	}

	// Zero-length integers are not allowed for R.
	if len_r == 0 {
		return false;
	}

	// Negative numbers are not allowed for R.
	if (sig[4] & 0x80) != 0 {
		return false;
	}

	// Null bytes at the start of R are not allowed, unless R would
	// otherwise be interpreted as a negative number.
	if len_r > 1 && sig[4] == 0 && (!(sig[5] & 0x80)) != 0 {
		return false;
	}

	// Check whether the S element is an integer.
	if sig[len_r + 4] != 2 {
		return false;
	}

	// Zero-length integers are not allowed for S.
	if len_s == 0 {
		return false;
	}

	// Negative numbers are not allowed for S.
	if (sig[len_r + 6] & 0x80) != 0 {
		return false;
	}

	// Null bytes at the start of S are not allowed, unless S would otherwise be
	// interpreted as a negative number.
	if len_s > 1 && (sig[len_r + 6] == 0) && (!(sig[len_r + 7] & 0x80)) != 0 {
		return false;
	}

	true
}

fn is_low_der_signature(sig: &[u8]) -> Result<bool, Error> {
	if !is_valid_signature_encoding(sig) {
		return Err(Error::SignatureDer);
	}

	let signature: Signature = sig.into();
	if !signature.check_low_s() {
		return Err(Error::SignatureHighS);
	}

	Ok(true)
}

fn is_defined_hashtype_signature(sig: &[u8]) -> bool {
	if sig.is_empty() {
		return false;
	}

	let n_hashtype = sig[sig.len() -1] & !(SignatureHash::AnyoneCanPay as u8);
	if n_hashtype < SignatureHash::All as u8 && n_hashtype > SignatureHash::Single as u8 {
		return false
	}
	true
}

fn check_signature_encoding(sig: &[u8], flags: &VerificationFlags) -> Result<bool, Error> {
	// Empty signature. Not strictly DER encoded, but allowed to provide a
	// compact way to provide an invalid signature for use with CHECK(MULTI)SIG

	if sig.is_empty() {
		return Ok(true);
	}

	if (flags.verify_dersig || flags.verify_low_s || flags.verify_strictenc) && !is_valid_signature_encoding(sig) {
		Err(Error::SignatureDer)
	} else if flags.verify_low_s && !try!(is_low_der_signature(sig)) {
		Ok(false)
	} else if flags.verify_strictenc && !is_defined_hashtype_signature(sig) {
		Err(Error::SignatureHashtype)
	} else {
		Ok(true)
	}
}

fn check_pubkey_encoding(v: &[u8], flags: &VerificationFlags) -> Result<bool, Error> {
	if flags.verify_strictenc && !is_public_key(v) {
		return Err(Error::PubkeyType);
	}

	Ok(true)
}

fn check_minimal_push(data: &[u8], opcode: Opcode) -> bool {
	if data.is_empty() {
		// Could have used OP_0.
		opcode == Opcode::OP_0
	} else if data.len() == 1 && data[0] >= 1 && data[0] <= 16 {
		// Could have used OP_1 .. OP_16.
		opcode as u8 == Opcode::OP_1 as u8 + (data[0] - 1)
	} else if data.len() == 1 && data[0] == 0x81 {
		// Could have used OP_1NEGATE
		opcode == Opcode::OP_1NEGATE
	} else if data.len() <= 75 {
		// Could have used a direct push (opcode indicating number of bytes pushed + those bytes).
		opcode as usize == data.len()
	} else if data.len() <= 255 {
		// Could have used OP_PUSHDATA.
		opcode == Opcode::OP_PUSHDATA1
	} else if data.len() <= 65535 {
		// Could have used OP_PUSHDATA2.
		opcode == Opcode::OP_PUSHDATA2
	} else {
		true
	}
}

pub fn eval_script(
	stack: &mut Vec<Vec<u8>>,
	script: &Script,
	flags: &VerificationFlags,
	checker: &SignatureChecker,
	_version: SignatureVersion
) -> Result<bool, Error> {
	if script.len() > script::MAX_SCRIPT_SIZE {
		return Err(Error::ScriptSize);
	}

	for i in script.into_iter() {
		match try!(i) {
			Instruction::PushValue(_opcode, num) => {
				stack.push(num.to_vec());
			},
			Instruction::PushBytes(opcode, bytes) => {
				// TODO: if fExec
				if flags.verify_minimaldata && !check_minimal_push(bytes, opcode) {
					return Err(Error::Minimaldata);
				}
				stack.push(bytes.to_vec());
			},
			Instruction::Normal(opcode) => match opcode {
				Opcode::OP_NOP => break,
				Opcode::OP_CHECKLOCKTIMEVERIFY => {
					if !flags.verify_clocktimeverify {
						if flags.verify_discourage_upgradable_nops {
							return Err(Error::DiscourageUpgradableNops);
						}
					}

					if stack.is_empty() {
						return Err(Error::InvalidStackOperation);
					}

					// Note that elsewhere numeric opcodes are limited to
					// operands in the range -2**31+1 to 2**31-1, however it is
					// legal for opcodes to produce results exceeding that
					// range. This limitation is implemented by CScriptNum's
					// default 4-byte limit.
					//
					// If we kept to that limit we'd have a year 2038 problem,
					// even though the nLockTime field in transactions
					// themselves is uint32 which only becomes meaningless
					// after the year 2106.
					//
					// Thus as a special case we tell CScriptNum to accept up
					// to 5-byte bignums, which are good until 2**39-1, well
					// beyond the 2**32-1 limit of the nLockTime field itself.
					let lock_time = try!(Num::from_slice(stack.last().unwrap(), flags.verify_minimaldata, 5));

					// In the rare event that the argument may be < 0 due to
					// some arithmetic being done first, you can always use
					// 0 MAX CHECKLOCKTIMEVERIFY.
					if lock_time.is_negative() {
						return Err(Error::NegativeLocktime);
					}

					if !checker.check_lock_time(lock_time) {
						return Err(Error::UnsatisfiedLocktime);
					}
				},
				Opcode::OP_NOP1 | Opcode::OP_NOP4 | Opcode::OP_NOP5 | Opcode::OP_NOP6 |
					Opcode::OP_NOP7 | Opcode::OP_NOP8 | Opcode::OP_NOP9 | Opcode::OP_NOP10 => {
					if flags.verify_discourage_upgradable_nops {
						return Err(Error::DiscourageUpgradableNops);
					}
				},
				Opcode::OP_RETURN => {
					return Err(Error::ReturnOpcode);
				},
				Opcode::OP_EQUAL => {
					if stack.len() < 2 {
						return Err(Error::InvalidStackOperation);
					}

					let equal = stack.pop() == stack.pop();
					let to_push = match equal {
						true => vec![1],
						false => vec![0],
					};
					stack.push(to_push);
				},
				Opcode::OP_EQUALVERIFY => {
					if stack.len() < 2 {
						return Err(Error::InvalidStackOperation);
					}

					let equal = stack.pop() == stack.pop();
					if !equal {
						return Err(Error::EqualVerify);
					}
				},
				_ => (),
			},
		}
	}

	let success = !stack.is_empty() && {
		let last = stack.last().unwrap();
		last != &vec![0; last.len()]
	};

	Ok(true)
}

#[cfg(test)]
mod tests {
	use hex::FromHex;
	use script::{Opcode, Script, VerificationFlags};
	use super::{is_public_key, eval_script, NoopSignatureChecker, SignatureVersion};

	#[test]
	fn tests_is_public_key() {
		assert!(!is_public_key(&[]));
		assert!(!is_public_key(&[1]));
		assert!(is_public_key(&"0495dfb90f202c7d016ef42c65bc010cd26bb8237b06253cc4d12175097bef767ed6b1fcb3caf1ed57c98d92e6cb70278721b952e29a335134857acd4c199b9d2f".from_hex().unwrap()));
		assert!(is_public_key(&[2; 33]));
		assert!(is_public_key(&[3; 33]));
		assert!(!is_public_key(&[4; 33]));
	}

	// https://github.com/bitcoin/bitcoin/blob/d612837814020ae832499d18e6ee5eb919a87907/src/test/script_tests.cpp#L900
	#[test]
	fn test_push_data() {
		let expected = vec![vec![0x5a]];
		let mut flags = VerificationFlags::default();
		let checker = NoopSignatureChecker;
		flags.verify_p2sh = true;
		let version = SignatureVersion::Base;
		let direct = Script::new(vec![Opcode::OP_PUSHBYTES_1 as u8, 0x5a]);
		let pushdata1 = Script::new(vec![Opcode::OP_PUSHDATA1 as u8, 0x1, 0x5a]);
		let pushdata2 = Script::new(vec![Opcode::OP_PUSHDATA2 as u8, 0x1, 0, 0x5a]);
		let pushdata4 = Script::new(vec![Opcode::OP_PUSHDATA4 as u8, 0x1, 0, 0, 0, 0x5a]);

		let mut direct_stack = vec![];
		let mut pushdata1_stack= vec![];
		let mut pushdata2_stack= vec![];
		let mut pushdata4_stack= vec![];
		assert!(eval_script(&mut direct_stack, &direct, &flags, &checker, version).unwrap());
		assert!(eval_script(&mut pushdata1_stack, &pushdata1, &flags, &checker, version).unwrap());
		assert!(eval_script(&mut pushdata2_stack, &pushdata2, &flags, &checker, version).unwrap());
		assert!(eval_script(&mut pushdata4_stack, &pushdata4, &flags, &checker, version).unwrap());

		assert_eq!(expected, direct_stack);
		assert_eq!(expected, pushdata1_stack);
		assert_eq!(expected, pushdata2_stack);
		assert_eq!(expected, pushdata4_stack);
	}
}
