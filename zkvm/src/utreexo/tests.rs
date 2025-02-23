use merlin::Transcript;

use super::*;
use crate::merkle::*;

impl MerkleItem for u64 {
    fn commit(&self, t: &mut Transcript) {
        t.append_u64(b"test_item", *self);
    }
}

#[test]
fn empty_utreexo() {
    let hasher = NodeHasher::<u64>::new();
    let forest0 = Forest::new();
    assert_eq!(
        forest0.root(&hasher),
        MerkleTree::empty_root(b"ZkVM.utreexo")
    );
}

#[test]
fn transient_items_utreexo() {
    let hasher = NodeHasher::new();
    let forest0 = Forest::new();

    let (_, _forest1, _catchup) = forest0
        .update(&hasher, |forest| {
            forest.insert(&0, &hasher);
            forest.insert(&1, &hasher);

            forest
                .delete(&1, Proof::Transient, &hasher)
                .expect("just received proof should not fail");
            forest
                .delete(&0, Proof::Transient, &hasher)
                .expect("just received proof should not fail");

            // double spends are not allowed
            assert_eq!(
                forest.delete(&1, Proof::Transient, &hasher),
                Err(UtreexoError::InvalidProof)
            );
            assert_eq!(
                forest.delete(&0, Proof::Transient, &hasher),
                Err(UtreexoError::InvalidProof)
            );

            Ok(())
        })
        .unwrap();
}

#[test]
fn insert_to_utreexo() {
    let hasher = NodeHasher::new();
    let forest0 = Forest::new();
    let (_, forest1, catchup1) = forest0
        .update(&hasher, |forest| {
            for i in 0..6 {
                forest.insert(&i, &hasher);
            }
            Ok(())
        })
        .expect("cannot fail");

    assert_eq!(
        forest1.root(&hasher),
        MerkleTree::root(b"ZkVM.utreexo", 0..6)
    );

    // update the proofs
    let proofs1 = (0..6)
        .map(|i| {
            catchup1
                .update_proof(&(i as u64), Proof::Transient, &hasher)
                .unwrap()
        })
        .collect::<Vec<_>>();

    // after the proofs were updated, deletions should succeed
    let _ = forest1
        .update(&hasher, |forest| {
            for i in 0..6u64 {
                forest.delete(&i, &proofs1[i as usize], &hasher)?;
            }
            Ok(())
        })
        .expect("all proofs must be valid");
}

#[test]
fn transaction_success() {
    let hasher = NodeHasher::new();
    let forest0 = Forest::new();
    let (_, forest1, catchup1) = forest0
        .update(&hasher, |forest| {
            for i in 0..6 {
                forest.insert(&i, &hasher);
            }
            Ok(())
        })
        .expect("cannot fail");

    // update the proofs
    let proofs1 = (0..6)
        .map(|i| {
            catchup1
                .update_proof(&(i as u64), Proof::Transient, &hasher)
                .unwrap()
        })
        .collect::<Vec<_>>();

    dbg!(proofs1.clone());

    let proofs1 = proofs1
        .into_iter()
        .enumerate()
        .map(|(i, p)| catchup1.update_proof(&(i as u64), p, &hasher).unwrap())
        .collect::<Vec<_>>();

    dbg!(proofs1.clone());

    //  d
    //  |\
    //  a   b   c
    //  |\  |\  |\
    //  0 1 2 3 4 5

    // We want to do several changes that would succeed, then do a failing transaction
    // and check that all pre-transaction changes were respected.

    let mut wf = forest1.work_forest();
    wf.insert(&6, &hasher);
    wf.delete(&0, &proofs1[0], &hasher)
        .expect("Should not fail.");

    //  d
    //  |\
    //  a   b   c   new
    //  |\  |\  |\  |
    //  x 1 2 3 4 5 6

    match wf.transaction::<_, (), ()>(|wf| {
        wf.insert(&7, &hasher);
        wf.insert(&8, &hasher);
        wf.delete(&7, &Proof::Transient, &hasher)
            .expect("Should not fail.");
        wf.delete(&1, &proofs1[1], &hasher)
            .expect("Should not fail.");
        Ok(())
    }) {
        Err(_) => {}
        Ok(_) => {}
    };

    let (new_forest, _) = wf.normalize(&hasher);

    //  d
    //  |\
    //  a   b   c   new
    //  |\  |\  |\  |\
    //  x x 2 3 4 5 6 8
    assert_eq!(
        new_forest.root(&hasher),
        MerkleTree::root(b"ZkVM.utreexo", &[2, 3, 4, 5, 6, 8])
    );
}

#[test]
fn transaction_fail() {
    let hasher = NodeHasher::new();
    let forest0 = Forest::new();
    let (_, forest1, catchup1) = forest0
        .update(&hasher, |forest| {
            for i in 0..6 {
                forest.insert(&i, &hasher);
            }
            Ok(())
        })
        .expect("cannot fail");

    // update the proofs
    let proofs1 = (0..6)
        .map(|i| {
            catchup1
                .update_proof(&(i as u64), Proof::Transient, &hasher)
                .unwrap()
        })
        .collect::<Vec<_>>();

    //  d
    //  |\
    //  a   b   c
    //  |\  |\  |\
    //  0 1 2 3 4 5

    // We want to do several changes that would succeed, then do a failing transaction
    // and check that all pre-transaction changes were respected.

    let mut wf = forest1.work_forest();
    wf.insert(&6, &hasher);
    wf.delete(&0, &proofs1[0], &hasher)
        .expect("Should not fail.");

    //  d
    //  |\
    //  a   b   c   new
    //  |\  |\  |\  |
    //  x 1 2 3 4 5 6

    match wf.transaction::<_, (), ()>(|wf| {
        wf.insert(&7, &hasher);
        wf.insert(&8, &hasher);
        wf.delete(&7, &Proof::Transient, &hasher)
            .expect("Should not fail.");
        wf.delete(&1, &proofs1[1], &hasher)
            .expect("Should not fail.");
        Err(())
    }) {
        Err(_) => {}
        Ok(_) => {}
    };

    let (new_forest, _) = wf.normalize(&hasher);

    // Should contain only the changes before transaction
    //  d                                         f
    //  |\                                        | \
    //  a   b   c  new   ->     b   c       ->    b   c   h
    //  |\  |\  |\  |           |\  |\            |\  |\  |\
    //  x 1 2 3 4 5 6         x 1 2 3 4 5 6       2 3 4 5 1 6
    assert_eq!(
        new_forest.root(&hasher),
        MerkleTree::root(b"ZkVM.utreexo", &[2, 3, 4, 5, 1, 6])
    );
}

#[test]
fn insert_and_delete_utreexo() {
    let n = 6u64;
    let hasher = NodeHasher::new();
    let forest0 = Forest::new();
    let (_, forest1, catchup1) = forest0
        .update(&hasher, |forest| {
            for i in 0..n {
                forest.insert(&i, &hasher);
            }
            Ok(())
        })
        .expect("cannot fail");

    // update the proofs
    let proofs1 = (0..n)
        .map(|i| {
            catchup1
                .update_proof(&(i as u64), Proof::Transient, &hasher)
                .unwrap()
        })
        .collect::<Vec<_>>();

    // after the proofs were updated, deletions should succeed

    forest1
        .verify(&0u64, &proofs1[0], &hasher)
        .expect("proof should be valid");
    forest1
        .verify(&5u64, &proofs1[5], &hasher)
        .expect("proof should be valid");

    fn verify_update<M: MerkleItem>(
        forest: &Forest,
        new_set: &[M],
        upd: impl FnOnce(&mut WorkForest),
    ) -> (Forest, Catchup) {
        let hasher = NodeHasher::<M>::new();
        let (_, forest2, catchup2) = forest
            .update(&hasher, |forest| {
                upd(forest);
                Ok(())
            })
            .unwrap();

        assert_eq!(
            forest2.root(&hasher),
            MerkleTree::root(b"ZkVM.utreexo", new_set)
        );

        (forest2, catchup2)
    }

    // delete 0:
    //  d                                       e
    //  |\                                      | \
    //  a   b   c      ->        b   c      ->  b   c
    //  |\  |\  |\               |\  |\         |\  |\
    //  0 1 2 3 4 5          x 1 2 3 4 5        2 3 4 5 1
    forest1.verify(&0u64, &proofs1[0], &hasher).unwrap();
    let (_, _) = verify_update(&forest1, &[2, 3, 4, 5, 1], |forest| {
        forest.delete(&0u64, &proofs1[0], &hasher).unwrap();
    });

    // delete 1:
    //  d                                       e
    //  |\                                      | \
    //  a   b   c      ->        b   c      ->  b   c
    //  |\  |\  |\               |\  |\         |\  |\
    //  0 1 2 3 4 5          0 x 2 3 4 5        2 3 4 5 0
    forest1.verify(&1u64, &proofs1[1], &hasher).unwrap();
    let (_, _) = verify_update(&forest1, &[2, 3, 4, 5, 0], |forest| {
        forest.delete(&1u64, &proofs1[1], &hasher).unwrap();
    });

    // delete 2:
    //  d                                       e
    //  |\                                      | \
    //  a   b   c      ->    a       c      ->  a   c
    //  |\  |\  |\           |\      |\         |\  |\
    //  0 1 2 3 4 5          0 1 x 3 4 5        0 1 4 5 3
    let (_, _) = verify_update(&forest1, &[0, 1, 4, 5, 3], |forest| {
        forest.delete(&2u64, &proofs1[2], &hasher).unwrap();
    });

    // delete 5:
    //  d                                       e
    //  |\                                      | \
    //  a   b   c      ->    a   b          ->  a   b
    //  |\  |\  |\           |\  |\             |\  |\
    //  0 1 2 3 4 5          0 1 2 3 4 x        0 1 2 3 4
    let (_, _) = verify_update(&forest1, &[0, 1, 2, 3, 4], |forest| {
        forest.delete(&5u64, &proofs1[5], &hasher).unwrap();
    });

    // delete 2,3:
    //  d                                       e
    //  |\                                      | \
    //  a   b   c      ->    a       c      ->  a   c
    //  |\  |\  |\           |\      |\         |\  |\
    //  0 1 2 3 4 5          0 1 x x 4 5        0 1 4 5
    let (_, _) = verify_update(&forest1, &[0, 1, 4, 5], |forest| {
        forest.delete(&2u64, &proofs1[2], &hasher).unwrap();
        forest.delete(&3u64, &proofs1[3], &hasher).unwrap();
    });

    // delete in another order
    let (_, _) = verify_update(&forest1, &[0, 1, 4, 5], |forest| {
        forest.delete(&3u64, &proofs1[3], &hasher).unwrap();
        forest.delete(&2u64, &proofs1[2], &hasher).unwrap();
    });

    // delete 0,3:
    //  d                                       f
    //  |\                                      | \
    //  a   b   c      ->            c      ->  e   c
    //  |\  |\  |\                   |\         |\  |\
    //  0 1 2 3 4 5          x 1 2 x 4 5        1 2 4 5
    let (_, _) = verify_update(&forest1, &[1, 2, 4, 5], |forest| {
        forest.delete(&0u64, &proofs1[0], &hasher).unwrap();
        forest.delete(&3u64, &proofs1[3], &hasher).unwrap();
    });

    // delete 0, insert 6, 7:
    //  d                                          f
    //  |\                                         | \
    //  a   b   c      ->        b   c       ->    b   c   h
    //  |\  |\  |\               |\  |\            |\  |\  |\
    //  0 1 2 3 4 5          x 1 2 3 4 5 6 7       2 3 4 5 1 6 7
    let (forest2, catchup) = verify_update(&forest1, &[2, 3, 4, 5, 1, 6, 7], |forest| {
        forest.delete(&0u64, &proofs1[0], &hasher).unwrap();
        forest.insert(&6u64, &hasher);
        forest.insert(&7u64, &hasher);
    });

    let proof7 = catchup
        .update_proof(&7u64, Proof::Transient, &hasher)
        .unwrap();
    let proof2 = catchup
        .update_proof(&2u64, proofs1[2].clone(), &hasher)
        .unwrap();

    // delete 2, 7:
    //   f                    f                   g
    //   | \                  | \                 | \
    //   b   c   h     ->     b   c   h     ->    c   h
    //   |\  |\  |\           |\  |\  |\          |\  |\
    //   2 3 4 5 1 6 7        x 3 4 5 1 6 x       4 5 1 6 3
    //
    let (_forest2, _catchup) = verify_update(&forest2, &[4, 5, 1, 6, 3], |forest| {
        forest.delete(&2u64, &proof2, &hasher).unwrap();
        forest.delete(&7u64, &proof7, &hasher).unwrap();
    });
}
