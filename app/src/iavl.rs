use std::cmp;

use integer_encoding::VarInt;
use sha2::{Digest, Sha256};

use crate::error::IAVLError;

#[derive(Debug)]
pub enum Node<'a> {
    Leaf(LeafNode<'a>),
    Inner(InnerNode<'a>),
}

#[derive(Debug)]
pub struct InnerNode<'a> {
    left_node: Box<Node<'a>>,
    right_node: Box<Node<'a>>,
    key: &'a [u8],
    height: u8,
    size: u32, // number of leaf nodes in this node's subtree
    left_hash: [u8; 32],
    right_hash: [u8; 32],
    version: u32,
}

#[derive(Debug)]
pub struct LeafNode<'a> {
    key: &'a [u8],
    value: &'a [u8],
    version: u32,
}

#[derive(Debug)]
pub struct IAVLTree<'a> {
    root: Node<'a>,
    version: u32,
}

impl<'a> Node<'a> {
    pub fn hash(&self) -> [u8; 32] {
        let serialized = self.serialize();
        Sha256::digest(serialized).into()
    }
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Node::Leaf(node) => {
                let height: i64 = 0; // i64 required for compatibility with cosmos
                let mut node_bytes = height.encode_var_vec();

                let size: i64 = 1; // i64 required for compatibility with cosmos
                node_bytes.append(&mut size.encode_var_vec());

                let version: i64 = node.version.into(); // conversion to i64 required for compatibility with cosmos
                node_bytes.append(&mut version.encode_var_vec());
                node_bytes.append(&mut encode_bytes(node.key.to_vec()));

                // Indirection is needed to provide proofs without values.
                let mut hasher = Sha256::new();
                hasher.update(node.value);
                let hashed_value = hasher.finalize();

                node_bytes.append(&mut encode_bytes(hashed_value.to_vec()));

                return node_bytes;
            }
            Node::Inner(node) => {
                let height: i64 = node.height.into(); // conversion to i64 required for compatibility with cosmos
                let mut node_bytes = height.encode_var_vec();

                let size: i64 = node.size.into(); // conversion to i64 required for compatibility with cosmos
                node_bytes.append(&mut size.encode_var_vec());

                let version: i64 = node.version.into(); // conversion to i64 required for compatibility with cosmos
                node_bytes.append(&mut version.encode_var_vec());

                node_bytes.append(&mut encode_bytes(node.left_hash.clone().into()));

                node_bytes.append(&mut encode_bytes(node.right_hash.clone().into()));

                return node_bytes;
            }
        }
    }
}

impl<'a> IAVLTree<'a> {
    pub fn new(key: &'a [u8], value: &'a [u8]) -> IAVLTree<'a> {
        IAVLTree {
            root: Node::Leaf(LeafNode {
                key,
                value,
                version: 1,
            }),
            version: 1,
        }
    }

    pub fn set(tree: IAVLTree<'a>, key: &'a [u8], value: &'a [u8]) -> IAVLTree<'a> {
        IAVLTree {
            root: Self::recursive_set(tree.root, key, value, tree.version),
            version: tree.version, // + 1,
        }
    }

    fn recursive_set(node: Node<'a>, key: &'a [u8], value: &'a [u8], version: u32) -> Node<'a> {
        match node {
            Node::Leaf(mut node) => {
                match key.cmp(&node.key) {
                    cmp::Ordering::Less => {
                        let left_node = Node::Leaf(LeafNode {
                            key,
                            value,
                            version,
                        });

                        let left_hash = left_node.hash();

                        let key = node.key;
                        let right_node = Node::Leaf(node);
                        let right_hash = right_node.hash();

                        let root = InnerNode {
                            key,
                            left_node: Box::new(left_node),
                            right_node: Box::new(right_node),
                            height: 1,
                            size: 2,
                            version,
                            left_hash,
                            right_hash,
                        };
                        return Node::Inner(root);
                    }
                    cmp::Ordering::Equal => {
                        node.value = value;
                        node.version = version;
                        return Node::Leaf(node);
                    }
                    cmp::Ordering::Greater => {
                        let right_node = Node::Leaf(LeafNode {
                            key,
                            value,
                            version,
                        });

                        let right_hash = right_node.hash();

                        let key = node.key;
                        let left_node = Node::Leaf(node);
                        let left_hash = left_node.hash();

                        let root = InnerNode {
                            key,
                            left_node: Box::new(left_node),
                            right_node: Box::new(right_node),
                            height: 1,
                            size: 2,
                            left_hash,
                            right_hash,
                            version,
                        };
                        return Node::Inner(root);
                    }
                };
            }
            Node::Inner(mut node) => {
                // Perform normal BST
                if key < node.key {
                    node.left_node =
                        Box::new(Self::recursive_set(*node.left_node, key, value, version));
                    node.left_hash = node.left_node.hash();
                } else {
                    node.right_node =
                        Box::new(Self::recursive_set(*node.right_node, key, value, version));
                    node.right_hash = node.right_node.hash();
                }

                // Update height
                node.height = 1 + cmp::max(
                    Self::get_height(&node.left_node),
                    Self::get_height(&node.right_node),
                );

                node.size = Self::get_size(&node.left_node) + Self::get_size(&node.right_node);

                // If the node is unbalanced then try out the usual four cases
                let balance_factor = Self::get_balance_factor(&node);
                if balance_factor > 1 {
                    match *node.left_node {
                        Node::Leaf(_) => {
                            panic!("Since balance factor > 1, expect this to be an inner node")
                        }
                        Node::Inner(left_node) => {
                            // Case 1 - Left Left
                            if key < left_node.key {
                                // move the left node back!
                                node.left_node = Box::new(Node::Inner(left_node));
                                return Node::Inner(
                                    Self::right_rotate(node, version)
                                        .expect("Expect rotation to always succeed"),
                                );
                            // Case 2 - Left Right
                            } else {
                                node.left_node = Box::new(Node::Inner(
                                    Self::left_rotate(left_node, version)
                                        .expect("Expect rotation to always succeed"),
                                ));
                                return Node::Inner(
                                    Self::right_rotate(node, version)
                                        .expect("Expect rotation to always succeed"),
                                );
                            }
                        }
                    }
                } else if balance_factor < -1 {
                    match *node.right_node {
                        Node::Leaf(_) => {
                            panic!("Since balance factor < -1, expect this to be an inner node")
                        }
                        Node::Inner(right_node) => {
                            // Case 3 - Right Right
                            if key > right_node.key {
                                // move the right node back!
                                node.right_node = Box::new(Node::Inner(right_node));
                                return Node::Inner(
                                    Self::left_rotate(node, version)
                                        .expect("Expect rotation to always succeed"),
                                );
                            //Case 4 - Right Left
                            } else {
                                node.right_node = Box::new(Node::Inner(
                                    Self::right_rotate(right_node, version)
                                        .expect("Expect rotation to always succeed"),
                                ));
                                return Node::Inner(
                                    Self::left_rotate(node, version)
                                        .expect("Expect rotation to always succeed"),
                                );
                            }
                        }
                    }
                }

                node.version = version;

                return Node::Inner(node);
            }
        };
    }

    fn get_height(node: &Node) -> u8 {
        match node {
            Node::Leaf(_) => 0,
            Node::Inner(n) => n.height,
        }
    }

    fn get_size(node: &Node) -> u32 {
        match node {
            Node::Leaf(_) => 1,
            Node::Inner(n) => n.size,
        }
    }

    fn get_balance_factor(node: &InnerNode) -> i16 {
        let left_height: i16 = Self::get_height(&node.left_node).into();
        let right_height: i16 = Self::get_height(&node.right_node).into();
        left_height - right_height
    }

    fn right_rotate(mut z: InnerNode, version: u32) -> Result<InnerNode, IAVLError> {
        let y = z.left_node;

        let mut y = match *y {
            Node::Inner(y) => y,
            Node::Leaf(_) => return Err(IAVLError::RotateError),
        };

        let t3 = y.right_node;

        // Perform rotation on z and update height and hash
        z.left_node = t3;
        z.height = 1 + cmp::max(
            Self::get_height(&z.left_node),
            Self::get_height(&z.right_node),
        );
        z.size = Self::get_size(&z.left_node) + Self::get_size(&z.right_node);
        z.version = version;
        z.left_hash = y.right_hash;
        let z = Node::Inner(z);

        // Perform rotation on y, update hash and update height
        y.right_hash = z.hash();
        y.right_node = Box::new(z);
        y.height = 1 + cmp::max(
            Self::get_height(&y.left_node),
            Self::get_height(&y.right_node),
        );
        y.size = Self::get_size(&y.left_node) + Self::get_size(&y.right_node);
        y.version = version;

        // Return the new root
        return Ok(y);
    }

    fn left_rotate(mut z: InnerNode, version: u32) -> Result<InnerNode, IAVLError> {
        let y = z.right_node;

        let mut y = match *y {
            Node::Inner(y) => y,
            Node::Leaf(_) => return Err(IAVLError::RotateError),
        };

        let t2 = y.left_node;

        // Perform rotation on z and update height and hash
        z.right_node = t2;
        z.height = 1 + cmp::max(
            Self::get_height(&z.left_node),
            Self::get_height(&z.right_node),
        );
        z.size = Self::get_size(&z.left_node) + Self::get_size(&z.right_node);
        z.version = version;
        z.right_hash = y.left_hash;
        let z = Node::Inner(z);

        // Perform rotation on y, update hash and update height
        y.left_hash = z.hash();
        y.left_node = Box::new(z);
        y.height = 1 + cmp::max(
            Self::get_height(&y.left_node),
            Self::get_height(&y.right_node),
        );
        y.size = Self::get_size(&y.left_node) + Self::get_size(&y.right_node);
        y.version = version;

        // Return the new root
        return Ok(y);
    }
}

fn encode_bytes(mut bz: Vec<u8>) -> Vec<u8> {
    let mut enc_bytes = bz.len().encode_var_vec();

    enc_bytes.append(&mut bz);

    return enc_bytes;
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn repeated_set_works() {
        let tree = IAVLTree::new(b"alice", b"abc");

        let tree = IAVLTree::set(tree, b"bob", b"123");
        let tree = IAVLTree::set(tree, b"c", b"1");
        let tree = IAVLTree::set(tree, b"q", b"1");

        let expected = [
            202, 52, 159, 10, 210, 166, 72, 207, 248, 190, 60, 114, 172, 147, 84, 27, 120, 202,
            189, 127, 230, 108, 58, 127, 251, 149, 9, 33, 87, 249, 158, 138,
        ];

        assert_eq!(expected, tree.root.hash());
    }
}
