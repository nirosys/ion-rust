use bumpalo::collections::Vec as BumpVec;
use bumpalo::Bump as BumpAllocator;

use crate::lazy::encoder::binary::v1_1::value_writer::BinaryValueWriter_1_1;
use crate::lazy::encoder::binary::v1_1::{flex_sym::FlexSym, flex_uint::FlexUInt};
use crate::lazy::encoder::value_writer::internal::{
    EExpWriterInternal, FieldEncoder, MakeValueWriter,
};
use crate::lazy::encoder::value_writer::{EExpWriter, SequenceWriter, StructWriter};
use crate::lazy::encoder::value_writer_config::ValueWriterConfig;
use crate::lazy::encoder::write_as_ion::WriteAsIon;
use crate::lazy::expanded::macro_table::MacroRef;
use crate::lazy::expanded::template::{Parameter, SignatureIterator};
use crate::raw_symbol_ref::AsRawSymbolRef;
use crate::{v1_1, ContextWriter, Encoding, IonResult, MacroTable, UInt};

/// A helper type that holds fields and logic that is common to [`BinaryListWriter_1_1`],
/// [`BinarySExpWriter_1_1`], and [`BinaryStructWriter_1_1`].
///
/// The [`BinaryContainerWriter_1_1`] does not know which type of container it is writing or whether that container
/// is length-prefixed or delimited. It only encodes provided values to its buffer. The owner of the
/// [`BinaryContainerWriter_1_1`] is responsible for:
///   * writing the correct opcode.
///   * copying the encoded data to the parent buffer (in the case of a length-prefixed container).
///   * writing field names to the [`BinaryContainerWriter_1_1`]'s buffer before each value (in the case of a struct).
pub(crate) struct BinaryContainerWriter_1_1<'value, 'top> {
    // An allocator reference that can be shared with nested container writers
    allocator: &'top BumpAllocator,
    encoder: ContainerEncodingKind<'value, 'top>,
    value_writer_config: ValueWriterConfig,
    macros: &'value MacroTable,
}

enum ContainerEncodingKind<'value, 'top> {
    Delimited(DelimitedEncoder<'value, 'top>),
    LengthPrefixed(LengthPrefixedEncoder<'value, 'top>),
}

impl<'top> ContainerEncodingKind<'_, 'top> {
    fn target_buffer(&mut self) -> &mut BumpVec<'top, u8> {
        match self {
            ContainerEncodingKind::Delimited(encoder) => encoder.buffer,
            ContainerEncodingKind::LengthPrefixed(encoder) => &mut encoder.child_values_buffer,
        }
    }
}

struct DelimitedEncoder<'value, 'top> {
    buffer: &'value mut BumpVec<'top, u8>,
}

struct LengthPrefixedEncoder<'value, 'top> {
    type_code: u8,
    flex_len_type_code: u8,
    parent_buffer: &'value mut BumpVec<'top, u8>,
    child_values_buffer: BumpVec<'top, u8>,
}

impl<'value, 'top> BinaryContainerWriter_1_1<'value, 'top> {
    const DELIMITED_END_OPCODE: u8 = 0xF0;

    pub fn new_delimited(
        start_opcode: u8,
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        write_options: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        buffer.push(start_opcode);
        let encoder = ContainerEncodingKind::Delimited(DelimitedEncoder { buffer });
        Self {
            allocator,
            encoder,
            value_writer_config: write_options,
            macros,
        }
    }

    pub fn new_length_prefixed(
        type_code: u8,
        flex_len_type_code: u8,
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        write_options: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const DEFAULT_CAPACITY: usize = 512;
        let encoder = ContainerEncodingKind::LengthPrefixed(LengthPrefixedEncoder {
            type_code,
            flex_len_type_code,
            parent_buffer: buffer,
            child_values_buffer: BumpVec::with_capacity_in(DEFAULT_CAPACITY, allocator),
        });
        Self {
            allocator,
            encoder,
            value_writer_config: write_options,
            macros,
        }
    }

    pub fn allocator(&self) -> &'top BumpAllocator {
        self.allocator
    }

    /// The buffer to which this ContainerWriter encodes child values.
    pub fn child_values_buffer(&mut self) -> &'_ mut BumpVec<'top, u8> {
        self.encoder.target_buffer()
    }

    pub fn config(&self) -> ValueWriterConfig {
        self.value_writer_config
    }

    /// Constructs a new [`BinaryValueWriter_1_1`] using this [`BinaryContainerWriter_1_1`]'s
    /// allocator and targeting its child values buffer.
    fn value_writer<'a>(&'a mut self) -> BinaryValueWriter_1_1<'a, 'top> {
        let value_writer_config = self.config();
        let macros = self.macros;
        // Create a value writer that will use the same container encodings it does by default
        BinaryValueWriter_1_1::new(
            self.allocator,
            self.child_values_buffer(),
            value_writer_config,
            macros,
        )
    }

    /// Encodes the provided `value` to the [`BinaryContainerWriter_1_1`]'s buffer.
    #[inline]
    pub fn write<V: WriteAsIon>(&mut self, value: V) -> IonResult<&mut Self> {
        let value_writer = self.value_writer();
        value.write_as_ion(value_writer)?;
        Ok(self)
    }

    pub fn end(self) -> IonResult<()> {
        match self.encoder {
            ContainerEncodingKind::Delimited(encoder) => {
                encoder.buffer.push(Self::DELIMITED_END_OPCODE)
            }
            ContainerEncodingKind::LengthPrefixed(encoder) => {
                let encoded_length = encoder.child_values_buffer.len();
                match encoded_length {
                    0..=15 => {
                        let opcode = encoder.type_code | encoded_length as u8;
                        encoder.parent_buffer.push(opcode);
                    }
                    _ => {
                        let opcode = encoder.flex_len_type_code;
                        encoder.parent_buffer.push(opcode);
                        FlexUInt::write(encoder.parent_buffer, encoded_length)?;
                    }
                }
                encoder
                    .parent_buffer
                    .extend_from_slice_copy(encoder.child_values_buffer.as_slice());
            }
        }
        Ok(())
    }
}

pub struct BinaryListWriter_1_1<'value, 'top> {
    pub(crate) container_writer: BinaryContainerWriter_1_1<'value, 'top>,
}

impl<'value, 'top> BinaryListWriter_1_1<'value, 'top> {
    pub(crate) fn with_container_writer(
        container_writer: BinaryContainerWriter_1_1<'value, 'top>,
    ) -> Self {
        Self { container_writer }
    }

    pub(crate) fn new_delimited(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const DELIMITED_LIST_OPCODE: u8 = 0xF1;
        let container_writer = BinaryContainerWriter_1_1::new_delimited(
            DELIMITED_LIST_OPCODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self::with_container_writer(container_writer)
    }

    pub(crate) fn new_length_prefixed(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const LENGTH_PREFIXED_LIST_TYPE_CODE: u8 = 0xB0;
        const LENGTH_PREFIXED_FLEX_LEN_LIST_TYPE_CODE: u8 = 0xFB;
        let container_writer = BinaryContainerWriter_1_1::new_length_prefixed(
            LENGTH_PREFIXED_LIST_TYPE_CODE,
            LENGTH_PREFIXED_FLEX_LEN_LIST_TYPE_CODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self::with_container_writer(container_writer)
    }
}

impl<'top> ContextWriter for BinaryListWriter_1_1<'_, 'top> {
    type NestedValueWriter<'a>
        = BinaryValueWriter_1_1<'a, 'top>
    where
        Self: 'a;
}

impl MakeValueWriter for BinaryListWriter_1_1<'_, '_> {
    fn make_value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        self.container_writer.value_writer()
    }
}

impl SequenceWriter for BinaryListWriter_1_1<'_, '_> {
    type Resources = ();

    fn write<V: WriteAsIon>(&mut self, value: V) -> IonResult<&mut Self> {
        self.container_writer.write(value)?;
        Ok(self)
    }

    fn close(self) -> IonResult<Self::Resources> {
        self.container_writer.end()
    }
}

pub struct BinarySExpWriter_1_1<'value, 'top> {
    pub(crate) container_writer: BinaryContainerWriter_1_1<'value, 'top>,
}

impl<'value, 'top> BinarySExpWriter_1_1<'value, 'top> {
    pub(crate) fn with_container_writer(
        container_writer: BinaryContainerWriter_1_1<'value, 'top>,
    ) -> Self {
        Self { container_writer }
    }

    pub(crate) fn new_delimited(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const DELIMITED_SEXP_OPCODE: u8 = 0xF2;
        let container_writer = BinaryContainerWriter_1_1::new_delimited(
            DELIMITED_SEXP_OPCODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self::with_container_writer(container_writer)
    }

    pub(crate) fn new_length_prefixed(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const LENGTH_PREFIXED_SEXP_TYPE_CODE: u8 = 0xC0;
        const LENGTH_PREFIXED_FLEX_LEN_SEXP_TYPE_CODE: u8 = 0xFC;
        let container_writer = BinaryContainerWriter_1_1::new_length_prefixed(
            LENGTH_PREFIXED_SEXP_TYPE_CODE,
            LENGTH_PREFIXED_FLEX_LEN_SEXP_TYPE_CODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self::with_container_writer(container_writer)
    }
}

impl<'top> ContextWriter for BinarySExpWriter_1_1<'_, 'top> {
    type NestedValueWriter<'a>
        = BinaryValueWriter_1_1<'a, 'top>
    where
        Self: 'a;
}

impl MakeValueWriter for BinarySExpWriter_1_1<'_, '_> {
    fn make_value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        let value_writer_config = self.container_writer.config();
        let macros = self.container_writer.macros;
        BinaryValueWriter_1_1::new(
            self.container_writer.allocator(),
            self.container_writer.child_values_buffer(),
            value_writer_config,
            macros,
        )
    }
}

impl SequenceWriter for BinarySExpWriter_1_1<'_, '_> {
    type Resources = ();

    fn write<V: WriteAsIon>(&mut self, value: V) -> IonResult<&mut Self> {
        self.container_writer.write(value)?;
        Ok(self)
    }

    fn close(self) -> IonResult<Self::Resources> {
        self.container_writer.end()
    }
}

pub struct BinaryStructWriter_1_1<'value, 'top> {
    // When true, struct field names are encoded as `FlexUInt`s.
    // When false, struct field names are encoded as `FlexSym`s.
    //
    // Always starts as `true`; remains `true` as long as field names being written
    // are symbol IDs. Once a field name with inline text needs to be encoded, switches to `false`.
    flex_uint_encoding: bool,
    container_writer: BinaryContainerWriter_1_1<'value, 'top>,
}

impl<'value, 'top> BinaryStructWriter_1_1<'value, 'top> {
    pub(crate) fn new_length_prefixed(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const LENGTH_PREFIXED_STRUCT_TYPE_CODE: u8 = 0xD0;
        const LENGTH_PREFIXED_FLEX_LEN_STRUCT_TYPE_CODE: u8 = 0xFD;
        let container_writer = BinaryContainerWriter_1_1::new_length_prefixed(
            LENGTH_PREFIXED_STRUCT_TYPE_CODE,
            LENGTH_PREFIXED_FLEX_LEN_STRUCT_TYPE_CODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self {
            flex_uint_encoding: true,
            container_writer,
        }
    }

    pub(crate) fn new_delimited(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
    ) -> Self {
        const DELIMITED_STRUCT_OPCODE: u8 = 0xF3;
        let container_writer = BinaryContainerWriter_1_1::new_delimited(
            DELIMITED_STRUCT_OPCODE,
            allocator,
            buffer,
            value_writer_config,
            macros,
        );
        Self {
            // Delimited structs always use FlexSym encoding.
            flex_uint_encoding: false,
            container_writer,
        }
    }

    pub(crate) fn fields_buffer(&mut self) -> &'_ mut BumpVec<'top, u8> {
        self.container_writer.child_values_buffer()
    }
}

impl FieldEncoder for BinaryStructWriter_1_1<'_, '_> {
    #[inline]
    fn encode_field_name(&mut self, name: impl AsRawSymbolRef) -> IonResult<()> {
        use crate::raw_symbol_ref::RawSymbolRef::*;

        let name_ref = name.as_raw_symbol_ref();
        match (self.flex_uint_encoding, name_ref) {
            // We're in FlexUInt encoding mode and can write this field without switching modes
            (true, SymbolId(sid)) if sid > 0 => {
                return FlexUInt::write(self.fields_buffer(), sid).map(|_| ());
            }
            // We're still in FlexUInt encoding mode, but this value requires FlexSym encoding
            (true, _) => {
                // Write the mode switch byte (FlexUInt 0 == 0x01)
                self.fields_buffer().push(0x01);
                self.flex_uint_encoding = false;
            }
            _ => {}
        }
        // At this point, we're in FlexSym encoding mode.
        FlexSym::encode_symbol(self.fields_buffer(), name_ref);
        Ok(())
    }
}

impl<'top> ContextWriter for BinaryStructWriter_1_1<'_, 'top> {
    type NestedValueWriter<'a>
        = BinaryValueWriter_1_1<'a, 'top>
    where
        Self: 'a;
}

impl MakeValueWriter for BinaryStructWriter_1_1<'_, '_> {
    fn make_value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        self.container_writer.value_writer()
    }
}

impl StructWriter for BinaryStructWriter_1_1<'_, '_> {
    fn close(mut self) -> IonResult<()> {
        if let ContainerEncodingKind::Delimited(_) = &mut self.container_writer.encoder {
            // Write the FlexSym escape (FlexUInt 0). The container writer can emit the closing
            // delimited END opcode.
            self.fields_buffer().push(0x01);
        }
        self.container_writer.end()
    }

    fn config(&self) -> ValueWriterConfig {
        v1_1::Binary::default_value_writer_config()
    }
}

pub struct BinaryEExpWriter_1_1<'value, 'top> {
    allocator: &'top BumpAllocator,
    buffer: &'value mut BumpVec<'top, u8>,
    value_writer_config: ValueWriterConfig,
    macros: &'value MacroTable,
    signature_iter: SignatureIterator<'value>,
    // TODO: Hold a reference to the macro signature and advance to the next parameter on
    //       each method call. Any time a value is written:
    //       * compare its type to the parameter to make sure it's legal.
    //       * see if the parameter's cardinality requires an update to the arg encoding bitmap
}

impl<'value, 'top> BinaryEExpWriter_1_1<'value, 'top> {
    pub fn new(
        allocator: &'top BumpAllocator,
        buffer: &'value mut BumpVec<'top, u8>,
        value_writer_config: ValueWriterConfig,
        macros: &'value MacroTable,
        invoked_macro: MacroRef<'value>,
    ) -> Self {
        let signature_iter = invoked_macro.iter_signature();
        Self {
            allocator,
            buffer,
            value_writer_config,
            macros,
            signature_iter,
        }
    }
}

impl<'top> ContextWriter for BinaryEExpWriter_1_1<'_, 'top> {
    type NestedValueWriter<'a>
        = BinaryValueWriter_1_1<'a, 'top>
    where
        Self: 'a;
}

impl MakeValueWriter for BinaryEExpWriter_1_1<'_, '_> {
    fn make_value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        BinaryValueWriter_1_1::new(
            self.allocator,
            self.buffer,
            self.value_writer_config,
            self.macros,
        )
    }
}

impl SequenceWriter for BinaryEExpWriter_1_1<'_, '_> {
    type Resources = ();

    fn close(self) -> IonResult<Self::Resources> {
        // Nothing to do
        // TODO: When we have length-prefixed macro invocations, this will require a step to flush the buffered encoding.
        Ok(())
    }
}

impl<'top> EExpWriterInternal for BinaryEExpWriter_1_1<'_, 'top> {
    fn expect_next_parameter(&mut self) -> IonResult<&Parameter> {
        self.signature_iter.expect_next_parameter()
    }
}

impl<'top> EExpWriter for BinaryEExpWriter_1_1<'_, 'top> {
    type ExprGroupWriter<'group>
        = BinaryExprGroupWriter<'group, 'top>
    where
        Self: 'group;

    fn invoked_macro(&self) -> MacroRef<'_> {
        self.signature_iter.parent_macro()
    }

    fn current_parameter(&self) -> Option<&Parameter> {
        self.signature_iter.current_parameter()
    }

    fn write_flex_uint(&mut self, value: impl Into<UInt>) -> IonResult<()> {
        FlexUInt::write(self.buffer, value)?;
        Ok(())
    }

    fn expr_group_writer(&mut self) -> IonResult<Self::ExprGroupWriter<'_>> {
        todo!("safe binary expression group serialization")
    }
}

pub struct BinaryExprGroupWriter<'group, 'top> {
    allocator: &'top BumpAllocator,
    buffer: &'group mut BumpVec<'top, u8>,
    value_writer_config: ValueWriterConfig,
    macros: &'group MacroTable,
}

impl<'top> ContextWriter for BinaryExprGroupWriter<'_, 'top> {
    type NestedValueWriter<'a>
        = BinaryValueWriter_1_1<'a, 'top>
    where
        Self: 'a;
}

impl MakeValueWriter for BinaryExprGroupWriter<'_, '_> {
    fn make_value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        BinaryValueWriter_1_1::new(
            self.allocator,
            self.buffer,
            self.value_writer_config,
            self.macros,
        )
    }
}

impl SequenceWriter for BinaryExprGroupWriter<'_, '_> {
    type Resources = ();

    fn close(self) -> IonResult<Self::Resources> {
        Ok(())
    }
}
