use crate::lazy::encoder::annotation_seq::{AnnotationSeq, AnnotationsVec};
use crate::lazy::encoder::value_writer::internal::{
    EExpWriterInternal, FieldEncoder, MakeValueWriter,
};
use crate::lazy::encoder::write_as_ion::WriteAsIon;
use crate::lazy::text::raw::v1_1::reader::MacroIdLike;
use crate::raw_symbol_ref::AsRawSymbolRef;
use crate::{Decimal, Int, IonResult, IonType, RawSymbolRef, Timestamp, UInt};

// This module is `pub(crate)` to deter crates from providing their own implementations of these traits.
pub(crate) mod internal {
    use crate::lazy::expanded::template::Parameter;
    use crate::raw_symbol_ref::AsRawSymbolRef;
    use crate::{ContextWriter, IonResult};

    pub trait MakeValueWriter: ContextWriter {
        /// Constructs a new instance of the implementation's preferred `ValueWriter` type.
        ///
        /// Instances of `ValueWriter` are single-use by design, as this allows the crate to
        /// statically guarantee that the application cannot (e.g.) try to encode multiple
        /// values after an annotations sequence or in a struct field.
        ///
        /// This trait (and this method by extension) are kept `pub(crate)` to prevent users
        /// from circumventing this restriction, creating and then using multiple value writers
        /// in situations where that would produce illegal data.
        fn make_value_writer(&mut self) -> <Self as ContextWriter>::NestedValueWriter<'_>;
    }

    /// A (private) prerequisite for [`StructWriter`](super::StructWriter) implementations.
    pub trait FieldEncoder {
        /// Encodes the field name portion of a field.
        ///
        /// For binary implementations, this is typically an encoding primitive (`VarUInt`,
        /// `FlexUInt`, or `FlexSym`).
        ///
        /// For text implementations, this typically includes indentation, a symbol token representing
        /// the field name itself, and the delimiting `:`.
        fn encode_field_name(&mut self, name: impl AsRawSymbolRef) -> IonResult<()>;
    }

    pub trait EExpWriterInternal {
        fn expect_next_parameter(&mut self) -> IonResult<&Parameter>;
    }
}

/// A writer which can encode nested values.
///
/// Implementors include top-level writers, container writers, and e-expression writers.
pub trait ContextWriter {
    /// The `ValueWriter` type family the implementor uses to encode data nested in this context.
    type NestedValueWriter<'a>: ValueWriter
    where
        Self: 'a;
}

pub trait EExpWriter: SequenceWriter + EExpWriterInternal {
    // TODO: more methods for writing tagless encodings
    type ExprGroupWriter<'group>: SequenceWriter
    where
        Self: 'group;

    fn invoked_macro(&self) -> MacroRef<'_>;

    fn current_parameter(&self) -> Option<&Parameter>;

    fn write_flex_uint(&mut self, _value: impl Into<UInt>) -> IonResult<()> {
        todo!("current only implemented for binary 1.1 to enable unit testing for the reader")
    }

    fn expr_group_writer(&mut self) -> IonResult<Self::ExprGroupWriter<'_>>;
}

pub trait AnnotatableWriter {
    type AnnotatedValueWriter<'a>: ValueWriter
    where
        Self: 'a;

    fn with_annotations<'a>(
        self,
        annotations: impl AnnotationSeq<'a>,
    ) -> IonResult<Self::AnnotatedValueWriter<'a>>
    where
        Self: 'a;
}

pub trait ValueWriter: AnnotatableWriter + Sized {
    type ListWriter: SequenceWriter<Resources = ()>;
    type SExpWriter: SequenceWriter<Resources = ()>;
    type StructWriter: StructWriter;
    type EExpWriter: EExpWriter<Resources = ()>;

    fn write_null(self, ion_type: IonType) -> IonResult<()>;
    fn write_bool(self, value: bool) -> IonResult<()>;
    fn write_i64(self, value: i64) -> IonResult<()>;
    fn write_int(self, value: &Int) -> IonResult<()>;
    fn write_f32(self, value: f32) -> IonResult<()>;
    fn write_f64(self, value: f64) -> IonResult<()>;
    fn write_decimal(self, value: &Decimal) -> IonResult<()>;
    fn write_timestamp(self, value: &Timestamp) -> IonResult<()>;
    fn write_string(self, value: impl AsRef<str>) -> IonResult<()>;
    fn write_symbol(self, value: impl AsRawSymbolRef) -> IonResult<()>;
    fn write_clob(self, value: impl AsRef<[u8]>) -> IonResult<()>;
    fn write_blob(self, value: impl AsRef<[u8]>) -> IonResult<()>;

    fn list_writer(self) -> IonResult<Self::ListWriter>;
    fn sexp_writer(self) -> IonResult<Self::SExpWriter>;
    fn struct_writer(self) -> IonResult<Self::StructWriter>;
    fn eexp_writer<'a>(self, macro_id: impl MacroIdLike<'a>) -> IonResult<Self::EExpWriter>
    where
        Self: 'a;

    fn write(self, value: impl WriteAsIon) -> IonResult<()> {
        value.write_as_ion(self)
    }

    fn write_list<V: WriteAsIon, I: IntoIterator<Item = V>>(self, values: I) -> IonResult<()> {
        let mut list = self.list_writer()?;
        list.write_all(values)?;
        list.close()
    }

    fn write_sexp<V: WriteAsIon, I: IntoIterator<Item = V>>(self, values: I) -> IonResult<()> {
        let mut sexp = self.sexp_writer()?;
        sexp.write_all(values)?;
        sexp.close()
    }

    fn write_struct<K: AsRawSymbolRef, V: WriteAsIon, I: IntoIterator<Item = (K, V)>>(
        self,
        values: I,
    ) -> IonResult<()> {
        let mut strukt = self.struct_writer()?;
        strukt.write_all(values)?;
        strukt.close()
    }
}

/// There are several implementations of `ValueWriter` that simply delegate calls to an expression.
/// This macro takes an expression and calls the `delegate!` proc macro on it for all of the methods
/// in the ValueWriter trait. For example:
/// ```text
///     delegate_value_writer_to!()     => delegate! { to self { ...signatures ... } }
///     delegate_value_writer_to!(foo)  => delegate! { to self.foo { ...signatures ... } }
///     delegate_value_writer_to!(0)    => delegate! { to self.0 { ...signatures ... } }
///     delegate_value_writer_to!(
///         closure
///         |self_: Self| {
///             self_.value_writer()
///         }
///     )                               => delegate! { to self.value_writer() { ...signatures ... } }
///     delegate_value_writer_to!(
///         fallible closure
///         |self_: Self| {
///             self_.returns_result()
///         }
///     )                               => delegate! { to self.returns_result()? { ...signatures ... } }
/// ```
///
/// Notice that if no parameter expression is passed, it results in delegation to `self`, which is helpful if
/// the trait is implemented by calling methods on the type's inherent impls.
///
/// Using this macro for such use cases centralizes the method signatures of ValueWriter, simplifying refactoring.
macro_rules! delegate_value_writer_to {
    // Declarative Rust macros (those defined with `macro_rules!`) cannot work with a `self` instance
    // from the enclosing context. Callers can pass `self` as an argument, but the macro's parameter
    // cannot be named `self`. The `delegate!` macro circumvents this by being a proc macro, which
    // does not have to adhere to the same macro hygiene rules as declarative macros.
    //
    // All of the patterns that this macro accepts are transformed into invocations of the final
    // `fallible closure` pattern, allowing us to only write out all of the trait method signatures
    // once.
    //
    // If no arguments are passed, trait method calls are delegated to inherent impl methods on `self`.
    () => {
        $crate::lazy::encoder::value_writer::delegate_value_writer_to!(closure std::convert::identity);
    };
    // If an identifier is passed, it is treated as the name of a subfield of `self`.
    ($name:ident) => {
       $crate::lazy::encoder::value_writer::delegate_value_writer_to!(closure |self_: Self| self_.$name);
    };
    // If a closure is provided, trait method calls are delegated to the closure's return value.
    (closure $f:expr) => {
        // In order to forward this call to the `fallible closure` pattern, the provided closure is
        // wrapped in another closure that wraps the closure's output in IonResult::Ok(_). The
        // compiler can eliminate the redundant closure call.
        $crate::lazy::encoder::value_writer::delegate_value_writer_to!(fallible closure |self_| {
            let infallible_closure = $f;
            $crate::IonResult::Ok(infallible_closure(self_))
        });
    };
    // If a fallible closure is provided, it will be called. If it returns an `Err`, the method
    // will return. Otherwise, trait method calls are delegated to the `Ok(_)` value.
    (fallible closure $f:expr) => {
        // The `self` keyword can only be used within the `delegate!` proc macro.
        delegate::delegate! {
            to {let f = $f; f(self)?} {
                fn write_null(self, ion_type: IonType) -> IonResult<()>;
                fn write_bool(self, value: bool) -> IonResult<()>;
                fn write_i64(self, value: i64) -> IonResult<()>;
                fn write_int(self, value: &Int) -> IonResult<()>;
                fn write_f32(self, value: f32) -> IonResult<()>;
                fn write_f64(self, value: f64) -> IonResult<()>;
                fn write_decimal(self, value: &Decimal) -> IonResult<()>;
                fn write_timestamp(self, value: &Timestamp) -> IonResult<()>;
                fn write_string(self, value: impl AsRef<str>) -> IonResult<()>;
                fn write_symbol(self, value: impl AsRawSymbolRef) -> IonResult<()>;
                fn write_clob(self, value: impl AsRef<[u8]>) -> IonResult<()>;
                fn write_blob(self, value: impl AsRef<[u8]>) -> IonResult<()>;
                fn list_writer(self) -> IonResult<Self::ListWriter>;
                fn sexp_writer(self) -> IonResult<Self::SExpWriter>;
                fn struct_writer(self) -> IonResult<Self::StructWriter>;
                fn eexp_writer<'a>(
                    self,
                    macro_id: impl MacroIdLike<'a>,
                 ) -> IonResult<Self::EExpWriter> where Self: 'a;

            }
        }
    };
}

/// [`delegate_value_writer_to`] allows you to omit arguments altogether, but that makes its effect
/// a bit unclear. This macro calls [`delegate_value_writer_to`] with no parameters but has a more
/// informative name.
macro_rules! delegate_value_writer_to_self {
    () => {
        $crate::lazy::encoder::value_writer::delegate_value_writer_to!();
    };
}

use crate::lazy::encoder::value_writer_config::ValueWriterConfig;
use crate::lazy::expanded::macro_table::MacroRef;
use crate::lazy::expanded::template::Parameter;
pub(crate) use delegate_value_writer_to;
pub(crate) use delegate_value_writer_to_self;

pub struct FieldWriter<'field, StructWriterType> {
    name: RawSymbolRef<'field>,
    struct_writer: &'field mut StructWriterType,
    // ValueWriterConfig is currently only meaningfully used in the binary 1.1 writer.
    // This generic `FieldWriter` type is used by all encodings, and so does not use the
    // `value_writer_config` field yet.
    #[allow(dead_code)]
    value_writer_config: ValueWriterConfig,
}

impl<'field, StructWriterType> FieldWriter<'field, StructWriterType> {
    pub(crate) fn new(
        name: RawSymbolRef<'field>,
        value_writer_config: ValueWriterConfig,
        struct_writer: &'field mut StructWriterType,
    ) -> Self {
        Self {
            name,
            struct_writer,
            value_writer_config,
        }
    }
}

impl<StructWriterType: StructWriter> AnnotatableWriter for FieldWriter<'_, StructWriterType> {
    type AnnotatedValueWriter<'a>
        = AnnotatedFieldWriter<'a, StructWriterType>
    where
        Self: 'a;

    fn with_annotations<'a>(
        self,
        annotations: impl AnnotationSeq<'a>,
    ) -> IonResult<Self::AnnotatedValueWriter<'a>>
    where
        Self: 'a,
    {
        Ok(AnnotatedFieldWriter::new(
            self.name,
            annotations,
            self.struct_writer,
        ))
    }
}

impl<'field, StructWriterType: StructWriter> ValueWriter for FieldWriter<'field, StructWriterType> {
    type ListWriter =
        <<StructWriterType as ContextWriter>::NestedValueWriter<'field> as ValueWriter>::ListWriter;
    type SExpWriter =
        <<StructWriterType as ContextWriter>::NestedValueWriter<'field> as ValueWriter>::SExpWriter;
    type StructWriter =
        <<StructWriterType as ContextWriter>::NestedValueWriter<'field> as ValueWriter>::StructWriter;
    type EExpWriter =
        <<StructWriterType as ContextWriter>::NestedValueWriter<'field> as ValueWriter>::EExpWriter;

    delegate_value_writer_to!(fallible closure |self_: Self| {
        self_.struct_writer.encode_field_name(self_.name)?;
        let value_writer = self_.struct_writer.make_value_writer();
        IonResult::Ok(value_writer)
    });
}

pub struct AnnotatedFieldWriter<'field, StructWriterType> {
    name: RawSymbolRef<'field>,
    annotations: AnnotationsVec<'field>,
    struct_writer: &'field mut StructWriterType,
}

impl<'field, StructWriterType: StructWriter> AnnotatedFieldWriter<'field, StructWriterType> {
    pub(crate) fn new(
        name: RawSymbolRef<'field>,
        annotations: impl AnnotationSeq<'field>,
        struct_writer: &'field mut StructWriterType,
    ) -> Self {
        Self {
            name,
            annotations: annotations.into_annotations_vec(),
            struct_writer,
        }
    }
}

impl<StructWriterType: StructWriter> AnnotatableWriter
    for AnnotatedFieldWriter<'_, StructWriterType>
{
    type AnnotatedValueWriter<'a>
        = AnnotatedFieldWriter<'a, StructWriterType>
    where
        Self: 'a;

    fn with_annotations<'a>(
        self,
        annotations: impl AnnotationSeq<'a>,
    ) -> IonResult<Self::AnnotatedValueWriter<'a>>
    where
        Self: 'a,
    {
        Ok(AnnotatedFieldWriter {
            name: self.name,
            annotations: annotations.into_annotations_vec(),
            struct_writer: self.struct_writer,
        })
    }
}

impl<'field, StructWriterType: StructWriter> ValueWriter
    for AnnotatedFieldWriter<'field, StructWriterType>
{
    type ListWriter =
        <<<StructWriterType as ContextWriter>::NestedValueWriter<'field> as AnnotatableWriter>::AnnotatedValueWriter<'field> as ValueWriter>::ListWriter;
    type SExpWriter =
    <<<StructWriterType as ContextWriter>::NestedValueWriter<'field> as AnnotatableWriter>::AnnotatedValueWriter<'field> as ValueWriter>::SExpWriter;
    type StructWriter =
    <<<StructWriterType as ContextWriter>::NestedValueWriter<'field> as AnnotatableWriter>::AnnotatedValueWriter<'field> as ValueWriter>::StructWriter;
    type EExpWriter =
    <<<StructWriterType as ContextWriter>::NestedValueWriter<'field> as AnnotatableWriter>::AnnotatedValueWriter<'field> as ValueWriter>::EExpWriter;

    delegate_value_writer_to!(fallible closure |self_: Self| {
        self_.struct_writer.encode_field_name(self_.name)?;
        let value_writer = self_.struct_writer.make_value_writer().with_annotations(self_.annotations)?;
        IonResult::Ok(value_writer)
    });
}

pub trait StructWriter: FieldEncoder + MakeValueWriter + Sized {
    /// Writes a struct field using the provided name/value pair.
    fn write<A: AsRawSymbolRef, V: WriteAsIon>(
        &mut self,
        name: A,
        value: V,
    ) -> IonResult<&mut Self> {
        self.encode_field_name(name)?;
        value.write_as_ion(self.make_value_writer())?;
        Ok(self)
    }

    fn write_all<A: AsRawSymbolRef, V: WriteAsIon, I: IntoIterator<Item = (A, V)>>(
        &mut self,
        fields: I,
    ) -> IonResult<&mut Self> {
        for field in fields {
            self.write(field.0, field.1)?;
        }
        Ok(self)
    }

    fn field_writer<'a>(&'a mut self, name: impl Into<RawSymbolRef<'a>>) -> FieldWriter<'a, Self> {
        FieldWriter::new(name.into(), self.config(), self)
    }
    fn close(self) -> IonResult<()>;

    fn config(&self) -> ValueWriterConfig;
}

/// Takes a series of `TYPE => METHOD` pairs, generating a function for each that calls the
/// corresponding value writer method and then returns `Ok(self)` upon success.
macro_rules! delegate_and_return_self {
    // End of iteration
    () => {};
    // Recurses one argument pair at a time
    ($value_type:ty => $method:ident, $($rest:tt)*) => {
        fn $method(&mut self, value: $value_type) -> IonResult<&mut Self> {
            self.value_writer().$method(value)?;
            Ok(self)
        }
        delegate_and_return_self!($($rest)*);
    };
}

pub trait SequenceWriter: MakeValueWriter {
    /// The type returned by the [`end`](Self::close) method.
    ///
    /// For top-level writers, this can be any resource(s) owned by the writer that need to survive
    /// after the writer is dropped. (For example, a `BufWriter` or `Vec` serving as the output.)
    ///
    /// Containers and E-expressions must use `()`.
    //  ^^^ This constraint could be loosened if needed, but it requires using verbose references
    //      to `<MyType as SequenceWriter>::End` in a variety of APIs.
    type Resources;

    fn value_writer(&mut self) -> Self::NestedValueWriter<'_> {
        <Self as MakeValueWriter>::make_value_writer(self)
    }

    /// Writes a value in the current context (list, s-expression, or stream) and upon success
    /// returns another reference to `self` to enable method chaining.
    fn write<V: WriteAsIon>(&mut self, value: V) -> IonResult<&mut Self> {
        value.write_as_ion(self.make_value_writer())?;
        Ok(self)
    }

    fn write_all<V: WriteAsIon, I: IntoIterator<Item = V>>(
        &mut self,
        values: I,
    ) -> IonResult<&mut Self> {
        for value in values {
            self.write(value)?;
        }
        Ok(self)
    }

    /// Closes out the sequence being written. Delimited writers can use this opportunity to emit
    /// a sentinel value, and length-prefixed writers can flush any buffered data to the output
    /// buffer.
    fn close(self) -> IonResult<Self::Resources>;

    // Creates functions that delegate to the ValueWriter method of the same name but which then
    // return `self` so it can be re-used/chained.
    delegate_and_return_self!(
        IonType => write_null,
        bool => write_bool,
        i64 => write_i64,
        &Int => write_int,
        f32 => write_f32,
        f64 => write_f64,
        &Decimal => write_decimal,
        &Timestamp => write_timestamp,
        impl AsRef<str> => write_string,
        impl AsRawSymbolRef => write_symbol,
        impl AsRef<[u8]> => write_clob,
        impl AsRef<[u8]> => write_blob,
    );

    fn list_writer(
        &mut self,
    ) -> IonResult<<Self::NestedValueWriter<'_> as ValueWriter>::ListWriter> {
        self.value_writer().list_writer()
    }

    fn sexp_writer(
        &mut self,
    ) -> IonResult<<Self::NestedValueWriter<'_> as ValueWriter>::SExpWriter> {
        self.value_writer().sexp_writer()
    }

    fn struct_writer(
        &mut self,
    ) -> IonResult<<Self::NestedValueWriter<'_> as ValueWriter>::StructWriter> {
        self.value_writer().struct_writer()
    }

    fn eexp_writer<'a>(
        &'a mut self,
        macro_id: impl MacroIdLike<'a>,
    ) -> IonResult<<Self::NestedValueWriter<'a> as ValueWriter>::EExpWriter> {
        self.value_writer().eexp_writer(macro_id)
    }

    fn write_list<V: WriteAsIon, I: IntoIterator<Item = V>>(
        &mut self,
        values: I,
    ) -> IonResult<&mut Self> {
        self.value_writer().write_list(values)?;
        Ok(self)
    }

    fn write_sexp<V: WriteAsIon, I: IntoIterator<Item = V>>(
        &mut self,
        values: I,
    ) -> IonResult<&mut Self> {
        self.value_writer().write_sexp(values)?;
        Ok(self)
    }

    fn write_struct<K: AsRawSymbolRef, V: WriteAsIon, I: IntoIterator<Item = (K, V)>>(
        &mut self,
        fields: I,
    ) -> IonResult<&mut Self> {
        self.value_writer().write_struct(fields)?;
        Ok(self)
    }
}

#[cfg(all(test, feature = "experimental-reader-writer"))]
mod tests {
    use crate::symbol_ref::AsSymbolRef;
    use crate::{ion_seq, v1_0, Element, IntoAnnotatedElement, SequenceWriter, Writer};
    use crate::{AnnotatableWriter, IonResult, ValueWriter};
    #[test]
    fn save_and_reuse_symbol_id() -> IonResult<()> {
        let mut writer = Writer::new(v1_0::Binary, vec![])?;
        let name_symbol = writer
            .value_writer()
            .symbol_table()
            .sid_for("name")
            .unwrap();
        writer
            // Write the symbol twice using its ID
            .write_symbol(name_symbol)?
            .write_symbol(name_symbol)?
            // Use the ID again as an annotation...
            .value_writer()
            .with_annotations(name_symbol)?
            // ...when writing the symbol once more.
            .write_symbol(name_symbol)?;
        let bytes = writer.close()?;
        let actual = Element::read_all(&bytes)?;
        let expected = ion_seq!(
            "name".as_symbol_ref()
            "name".as_symbol_ref()
            "name".as_symbol_ref().with_annotations(["name"])
        );
        assert_eq!(actual, expected);
        Ok(())
    }
}
