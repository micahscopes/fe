# C2C borrow-checking reproductions

`ice_min.fe` and `c2c_loop_ice.fe` report `16-0004` during `fe test` on both
mb3 and mainline `e2fa7e53c`. They live below the top-level `fe_test` fixture
directory so the passing `dir_test` gate does not run them. Keep them here until
the upstream ownership and normalized-IR verifier issue is fixed, then promote
them to ordinary `fe_test` fixtures.
