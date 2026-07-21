//! Native-layout serialization for generated numeric parser tables.
//!
//! This module owns the physical file format. The runtime continues to consume
//! its existing native slices and token records directly.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::LitStr;

use crate::token::{
    EncodedTokenAccept, EncodedTokenEdge, EncodedTokenEof, EncodedTokenState, EncodedTokenTable,
};

const U16_SIZE: usize = size_of::<u16>();
const U32_SIZE: usize = size_of::<u32>();
const TOKEN_STATE_SIZE: usize = size_of::<rezel_lr::TokenState>();
const TOKEN_ACCEPT_SIZE: usize = size_of::<rezel_lr::TokenAccept>();
const TOKEN_EDGE_SIZE: usize = size_of::<rezel_lr::TokenEdge>();
const TOKEN_EOF_SIZE: usize = size_of::<rezel_lr::TokenEof>();
const DYNAMIC_PRECEDENCE_SIZE: usize = size_of::<rezel_lr::DynamicPrecedence>();
const GENERATED_TABLES_ALIGNMENT: usize = 64;

pub(crate) struct TokenTableFields {
    pub(crate) states: Ident,
    pub(crate) accepts: Ident,
    pub(crate) edges: Ident,
    pub(crate) eof: Ident,
}

pub(crate) struct BinaryTableOutput {
    pub(crate) declaration: TokenStream,
    pub(crate) little_endian: Vec<u8>,
    pub(crate) big_endian: Vec<u8>,
}

struct BinaryField {
    name: Ident,
    element_type: TokenStream,
    length: usize,
}

#[derive(Default)]
pub(crate) struct BinaryTables {
    fields: Vec<BinaryField>,
    little_endian: Vec<u8>,
    big_endian: Vec<u8>,
    alignment: usize,
}

impl BinaryTables {
    pub(crate) fn push_u16(&mut self, name: &str, values: &[u16]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * U16_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * U16_SIZE);
        for value in values {
            little_endian.extend_from_slice(&value.to_le_bytes());
            big_endian.extend_from_slice(&value.to_be_bytes());
        }
        self.push_field(
            name,
            quote!(u16),
            align_of::<u16>(),
            U16_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    pub(crate) fn push_u32(&mut self, name: &str, values: &[u32]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * U32_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * U32_SIZE);
        for value in values {
            little_endian.extend_from_slice(&value.to_le_bytes());
            big_endian.extend_from_slice(&value.to_be_bytes());
        }
        self.push_field(
            name,
            quote!(u32),
            align_of::<u32>(),
            U32_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    pub(crate) fn push_token_table(
        &mut self,
        prefix: &str,
        table: &EncodedTokenTable,
    ) -> TokenTableFields {
        let states = self.push_token_states(&format!("{prefix}_states"), &table.states);
        let accepts = self.push_token_accepts(&format!("{prefix}_accepts"), &table.accepts);
        let edges = self.push_token_edges(&format!("{prefix}_edges"), &table.edges);
        let eof = self.push_token_eof(&format!("{prefix}_eof"), &table.eof);
        TokenTableFields {
            states,
            accepts,
            edges,
            eof,
        }
    }

    pub(crate) fn push_dynamic_precedences(&mut self, name: &str, values: &[(u16, i16)]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * DYNAMIC_PRECEDENCE_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * DYNAMIC_PRECEDENCE_SIZE);
        for (term, value) in values {
            little_endian.extend_from_slice(&term.to_le_bytes());
            little_endian.extend_from_slice(&value.to_le_bytes());
            big_endian.extend_from_slice(&term.to_be_bytes());
            big_endian.extend_from_slice(&value.to_be_bytes());
        }
        self.push_field(
            name,
            quote!(rezel_lr::DynamicPrecedence),
            align_of::<rezel_lr::DynamicPrecedence>(),
            DYNAMIC_PRECEDENCE_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    pub(crate) fn finish(
        mut self,
        little_endian_path: &str,
        big_endian_path: &str,
    ) -> BinaryTableOutput {
        let alignment = self.alignment.max(GENERATED_TABLES_ALIGNMENT);
        pad_to(&mut self.little_endian, alignment);
        pad_to(&mut self.big_endian, alignment);
        assert_eq!(self.little_endian.len(), self.big_endian.len());

        let fields = self.fields.iter().map(|field| {
            let name = &field.name;
            let element_type = &field.element_type;
            let length = field.length;
            quote!(#name: [#element_type; #length])
        });
        let little_endian_path = LitStr::new(little_endian_path, Span::call_site());
        let big_endian_path = LitStr::new(big_endian_path, Span::call_site());
        let declaration = quote! {
            #[repr(C, align(64))]
            #[derive(zerocopy::FromBytes, zerocopy::Immutable)]
            struct GeneratedTables {
                #(#fields,)*
            }

            #[cfg(target_endian = "little")]
            static TABLES_LE: GeneratedTables =
                rezel_lr::__private::include_value!(#little_endian_path);
            #[cfg(target_endian = "big")]
            static TABLES_BE: GeneratedTables =
                rezel_lr::__private::include_value!(#big_endian_path);
            #[cfg(target_endian = "little")]
            use TABLES_LE as TABLES;
            #[cfg(target_endian = "big")]
            use TABLES_BE as TABLES;
        };
        BinaryTableOutput {
            declaration,
            little_endian: self.little_endian,
            big_endian: self.big_endian,
        }
    }

    fn push_token_states(&mut self, name: &str, values: &[EncodedTokenState]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * TOKEN_STATE_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * TOKEN_STATE_SIZE);
        for value in values {
            little_endian.extend_from_slice(&value.group_mask.to_le_bytes());
            little_endian.extend_from_slice(&value.accept_start.to_le_bytes());
            little_endian.extend_from_slice(&value.edge_start.to_le_bytes());
            little_endian.push(value.accept_count);
            little_endian.push(value.edge_count);

            big_endian.extend_from_slice(&value.group_mask.to_be_bytes());
            big_endian.extend_from_slice(&value.accept_start.to_be_bytes());
            big_endian.extend_from_slice(&value.edge_start.to_be_bytes());
            big_endian.push(value.accept_count);
            big_endian.push(value.edge_count);
        }
        self.push_field(
            name,
            quote!(rezel_lr::TokenState),
            align_of::<rezel_lr::TokenState>(),
            TOKEN_STATE_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    fn push_token_accepts(&mut self, name: &str, values: &[EncodedTokenAccept]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * TOKEN_ACCEPT_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * TOKEN_ACCEPT_SIZE);
        for value in values {
            little_endian.extend_from_slice(&value.term.to_le_bytes());
            little_endian.extend_from_slice(&value.group_mask.to_le_bytes());
            big_endian.extend_from_slice(&value.term.to_be_bytes());
            big_endian.extend_from_slice(&value.group_mask.to_be_bytes());
        }
        self.push_field(
            name,
            quote!(rezel_lr::TokenAccept),
            align_of::<rezel_lr::TokenAccept>(),
            TOKEN_ACCEPT_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    fn push_token_edges(&mut self, name: &str, values: &[EncodedTokenEdge]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * TOKEN_EDGE_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * TOKEN_EDGE_SIZE);
        let value_size = U32_SIZE * 2 + U16_SIZE;
        let padding = TOKEN_EDGE_SIZE
            .checked_sub(value_size)
            .expect("TokenEdge contains its encoded fields");
        for value in values {
            little_endian.extend_from_slice(&value.from.to_le_bytes());
            little_endian.extend_from_slice(&value.to.to_le_bytes());
            little_endian.extend_from_slice(&value.target.to_le_bytes());
            little_endian.resize(little_endian.len() + padding, 0);

            big_endian.extend_from_slice(&value.from.to_be_bytes());
            big_endian.extend_from_slice(&value.to.to_be_bytes());
            big_endian.extend_from_slice(&value.target.to_be_bytes());
            big_endian.resize(big_endian.len() + padding, 0);
        }
        self.push_field(
            name,
            quote!(rezel_lr::TokenEdge),
            align_of::<rezel_lr::TokenEdge>(),
            TOKEN_EDGE_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    fn push_token_eof(&mut self, name: &str, values: &[EncodedTokenEof]) -> Ident {
        let mut little_endian = Vec::with_capacity(values.len() * TOKEN_EOF_SIZE);
        let mut big_endian = Vec::with_capacity(values.len() * TOKEN_EOF_SIZE);
        for value in values {
            little_endian.extend_from_slice(&value.state.to_le_bytes());
            little_endian.extend_from_slice(&value.target.to_le_bytes());
            big_endian.extend_from_slice(&value.state.to_be_bytes());
            big_endian.extend_from_slice(&value.target.to_be_bytes());
        }
        self.push_field(
            name,
            quote!(rezel_lr::TokenEof),
            align_of::<rezel_lr::TokenEof>(),
            TOKEN_EOF_SIZE,
            values.len(),
            little_endian,
            big_endian,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn push_field(
        &mut self,
        name: &str,
        element_type: TokenStream,
        alignment: usize,
        element_size: usize,
        length: usize,
        mut little_endian: Vec<u8>,
        mut big_endian: Vec<u8>,
    ) -> Ident {
        // These operations mirror the field-placement rules of the generated
        // repr(C) aggregate. Element encoders include any record-internal
        // padding, while this function inserts padding between aggregate fields.
        assert!(alignment.is_power_of_two());
        assert_eq!(little_endian.len(), element_size * length);
        assert_eq!(big_endian.len(), element_size * length);
        assert!(
            self.fields.iter().all(|field| field.name != name),
            "duplicate generated binary field {name}"
        );

        pad_to(&mut self.little_endian, alignment);
        pad_to(&mut self.big_endian, alignment);
        self.little_endian.append(&mut little_endian);
        self.big_endian.append(&mut big_endian);
        self.alignment = self.alignment.max(alignment);

        let name = Ident::new(name, Span::call_site());
        self.fields.push(BinaryField {
            name: name.clone(),
            element_type,
            length,
        });
        name
    }
}

fn pad_to(bytes: &mut Vec<u8>, alignment: usize) {
    if alignment == 0 {
        return;
    }
    let remainder = bytes.len() % alignment;
    if remainder != 0 {
        bytes.resize(bytes.len() + alignment - remainder, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_native_layout_for_both_byte_orders() {
        let mut tables = BinaryTables::default();
        tables.push_u16("shorts", &[0x1234]);
        tables.push_u32("words", &[0x1234_5678]);
        let output = tables.finish("tables.le.bin", "tables.be.bin");

        assert_eq!(output.little_endian.len(), GENERATED_TABLES_ALIGNMENT);
        assert_eq!(output.big_endian.len(), GENERATED_TABLES_ALIGNMENT);
        assert_eq!(
            &output.little_endian[..8],
            &[0x34, 0x12, 0, 0, 0x78, 0x56, 0x34, 0x12]
        );
        assert_eq!(
            &output.big_endian[..8],
            &[0x12, 0x34, 0, 0, 0x12, 0x34, 0x56, 0x78]
        );
        assert!(output.little_endian[8..].iter().all(|byte| *byte == 0));
        assert!(output.big_endian[8..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn writes_token_edge_padding() {
        let mut tables = BinaryTables::default();
        let table = EncodedTokenTable {
            edges: vec![EncodedTokenEdge {
                from: 0x1234,
                to: 0x10_ffff,
                target: 7,
            }],
            ..EncodedTokenTable::default()
        };
        tables.push_token_table("token", &table);
        let output = tables.finish("tables.le.bin", "tables.be.bin");

        assert_eq!(output.little_endian.len(), GENERATED_TABLES_ALIGNMENT);
        assert_eq!(
            &output.little_endian[..12],
            &[0x34, 0x12, 0, 0, 0xff, 0xff, 0x10, 0, 7, 0, 0, 0]
        );
        assert!(output.little_endian[12..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn writes_sparse_dynamic_precedences_for_both_byte_orders() {
        let mut tables = BinaryTables::default();
        tables.push_dynamic_precedences("dynamic", &[(0x1234, -2)]);
        let output = tables.finish("tables.le.bin", "tables.be.bin");

        assert_eq!(&output.little_endian[..4], &[0x34, 0x12, 0xfe, 0xff]);
        assert_eq!(&output.big_endian[..4], &[0x12, 0x34, 0xff, 0xfe]);
    }
}
