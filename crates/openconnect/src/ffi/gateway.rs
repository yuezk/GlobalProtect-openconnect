use std::{
  ffi::{c_char, c_int},
  ptr,
};

/// Resolve with the system's normal policy, then prefer one family without
/// discarding alternatives. The returned list remains owned by libc and is
/// released by OpenConnect with freeaddrinfo.
///
/// # Safety
/// Arguments must satisfy getaddrinfo's pointer and lifetime requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vpn_resolve_gateway(
  node: *const c_char,
  service: *const c_char,
  hints: *const libc::addrinfo,
  result: *mut *mut libc::addrinfo,
  preference: u32,
) -> c_int {
  // SAFETY: The OpenConnect callback forwards valid getaddrinfo arguments.
  let status = unsafe { libc::getaddrinfo(node, service, hints, result) };
  if status != 0 {
    return status;
  }
  let family = match preference {
    4 => libc::AF_INET,
    6 => libc::AF_INET6,
    _ => return 0,
  };
  // SAFETY: A successful getaddrinfo call returns an owned, writable list.
  unsafe { *result = prefer_family(*result, family) };
  0
}

/// # Safety
/// `head` must be an exclusively owned, writable addrinfo list with no cycles.
unsafe fn prefer_family(head: *mut libc::addrinfo, family: c_int) -> *mut libc::addrinfo {
  let mut addresses = Vec::new();
  let mut current = head;
  // SAFETY: The caller owns every node; sorting only changes the list links.
  unsafe {
    while !current.is_null() {
      addresses.push(current);
      current = (*current).ai_next;
    }
    addresses.sort_by_key(|address| (**address).ai_family != family);
    for (index, address) in addresses.iter().enumerate() {
      (**address).ai_next = addresses.get(index + 1).copied().unwrap_or(ptr::null_mut());
    }
  }
  addresses.first().copied().unwrap_or(ptr::null_mut())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ordered_indices(families: &[c_int], preferred: c_int) -> Vec<usize> {
    let mut addresses: Vec<libc::addrinfo> = families
      .iter()
      .enumerate()
      .map(|(index, family)| {
        // SAFETY: All-zero addrinfo has null pointers; these fixtures own no OS data.
        let mut address: libc::addrinfo = unsafe { std::mem::zeroed() };
        address.ai_family = *family;
        address.ai_flags = index as c_int;
        address
      })
      .collect();
    for index in 1..addresses.len() {
      addresses[index - 1].ai_next = &mut addresses[index];
    }
    let head = if addresses.is_empty() {
      ptr::null_mut()
    } else {
      addresses.as_mut_ptr()
    };
    // SAFETY: The fixture owns stable nodes throughout sorting and traversal.
    unsafe {
      let mut current = prefer_family(head, preferred);
      let mut indices = Vec::new();
      while !current.is_null() {
        assert!(indices.len() < addresses.len(), "address list contains a cycle");
        indices.push((*current).ai_flags as usize);
        current = (*current).ai_next;
      }
      indices
    }
  }

  #[test]
  fn preferences_retain_all_addresses_and_order_within_each_family() {
    let mixed = [libc::AF_INET6, libc::AF_INET, libc::AF_INET6, libc::AF_INET];
    assert_eq!(ordered_indices(&mixed, libc::AF_INET), [1, 3, 0, 2]);
    assert_eq!(ordered_indices(&mixed, libc::AF_INET6), [0, 2, 1, 3]);
    assert_eq!(
      ordered_indices(&[libc::AF_INET6, libc::AF_INET6], libc::AF_INET),
      [0, 1]
    );
    assert_eq!(ordered_indices(&[libc::AF_INET, libc::AF_INET], libc::AF_INET6), [0, 1]);
    assert!(ordered_indices(&[], libc::AF_INET).is_empty());
  }

  #[test]
  fn native_lookup_preserves_errors_and_returns_freeable_results() {
    let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
    hints.ai_socktype = libc::SOCK_STREAM;
    hints.ai_flags = libc::AI_NUMERICHOST;
    for preference in [0, 4, 6] {
      let mut result = ptr::null_mut();
      // SAFETY: Static NUL-terminated inputs, initialized hints and a writable output.
      unsafe {
        assert_ne!(
          vpn_resolve_gateway(c"invalid".as_ptr(), c"443".as_ptr(), &hints, &mut result, preference),
          0
        );
        assert_eq!(
          vpn_resolve_gateway(c"::1".as_ptr(), c"443".as_ptr(), &hints, &mut result, preference),
          0
        );
        assert!(!result.is_null());
        assert_eq!((*result).ai_family, libc::AF_INET6);
        libc::freeaddrinfo(result);
      }
    }
  }
}
