import karva


def check_os_error(error: OSError) -> bool:
    return error.errno == 13


def test_raises_accepts_narrow_check() -> None:
    with karva.raises(OSError, check=check_os_error):
        raise OSError(13, "permission denied")
