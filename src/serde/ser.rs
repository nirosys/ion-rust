use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use serde::ser::Impossible;
use serde::{ser, Serialize};

use crate::lazy::encoder::value_writer::internal::{FieldEncoder, MakeValueWriter};
use crate::lazy::encoder::value_writer::{SequenceWriter, StructWriter, ValueWriter};
use crate::lazy::encoder::writer::Writer;
use crate::lazy::encoding::{BinaryEncoding_1_0, Encoding, TextEncoding_1_0};
use crate::result::IonFailure;
use crate::serde::decimal::TUNNELED_DECIMAL_TYPE_NAME;
use crate::serde::timestamp::TUNNELED_TIMESTAMP_TYPE_NAME;
use crate::symbol_ref::AsSymbolRef;
use crate::write_config::{WriteConfig, WriteConfigKind};
use crate::Value::Null;
use crate::{Decimal, IonError, IonResult, IonType, TextFormat, Timestamp};

fn write_with_config<T: Serialize, E: Encoding>(
    value: &T,
    config: WriteConfig<E>,
) -> IonResult<Vec<u8>> {
    let is_human_readable = matches!(config.kind, WriteConfigKind::Text(_));
    let mut writer = Writer::new(config, vec![])?;
    let serializer = ValueSerializer::new(writer.value_writer(), is_human_readable);
    value.serialize(serializer)?;
    writer.close()
}

// TODO: Break these into modules for 1.0 and 1.1

/// Serialize an object into pretty formatted Ion text
pub fn to_pretty<T>(value: &T) -> IonResult<String>
where
    T: Serialize,
{
    let config = WriteConfig::<TextEncoding_1_0>::new(TextFormat::Pretty);
    let bytes = write_with_config(value, config)?;
    match String::from_utf8(bytes) {
        Ok(data) => Ok(data),
        Err(e) => IonResult::encoding_error(e.to_string()),
    }
}

/// Serialize an object into compact Ion text format
pub fn to_string<T>(value: &T) -> IonResult<String>
where
    T: Serialize,
{
    let config = WriteConfig::<TextEncoding_1_0>::new(TextFormat::Compact);
    let bytes = write_with_config(value, config)?;
    match String::from_utf8(bytes) {
        Ok(data) => Ok(data),
        Err(e) => IonResult::encoding_error(e.to_string()),
    }
}

/// Serialize an object into Ion binary format
pub fn to_binary<T>(value: &T) -> IonResult<Vec<u8>>
where
    T: Serialize,
{
    let config = WriteConfig::<BinaryEncoding_1_0>::new();
    write_with_config(value, config)
}

/// Implements a standard serializer for Ion
pub struct ValueSerializer<'a, V: ValueWriter> {
    pub(crate) value_writer: V,
    pub(crate) is_human_readable: bool,
    pub(crate) annotations: Vec<&'static str>,
    lifetime: PhantomData<&'a ()>,
}

impl<V: ValueWriter> ValueSerializer<'_, V> {
    pub fn new(value_writer: V, is_human_readable: bool) -> Self {
        Self {
            value_writer,
            is_human_readable,
            annotations: vec!(),
            lifetime: PhantomData,
        }
    }
}

impl<'a, V: ValueWriter + 'a> ser::Serializer for ValueSerializer<'a, V> {
    type Ok = ();
    type Error = IonError;

    type SerializeSeq = SeqWriter<V>;
    type SerializeTuple = SeqWriter<V::AnnotatedValueWriter<'a>>;
    type SerializeTupleStruct = SeqWriter<V::AnnotatedValueWriter<'a>>;
    type SerializeTupleVariant = SeqWriter<V::AnnotatedValueWriter<'a>>;
    type SerializeMap = MapWriter<V>;
    type SerializeStruct = MapWriter<V>;
    type SerializeStructVariant = MapWriter<V::AnnotatedValueWriter<'a>>;

    /// Determine whether Serialize implementations should serialize in human-readable form.
    /// For binary Ion this will return `false` and for text Ion this will return `true`.
    fn is_human_readable(&self) -> bool {
        self.is_human_readable
    }

    /// Serialize a boolean to a bool value
    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.value_writer.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v as i64)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v as i64)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    /// Serialize all integer types using the `Integer` intermediary type.
    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.value_writer.write(v)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        // TODO: This could be optimized.
        self.value_writer.with_annotations(self.annotations)?.write(v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(v)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(Null(IonType::Null))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_none()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(name.as_symbol_ref())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.value_writer.with_annotations(self.annotations)?.write(variant.as_symbol_ref())
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        if name == TUNNELED_TIMESTAMP_TYPE_NAME {
            assert_eq!(
                std::mem::size_of_val(value),
                std::mem::size_of::<Timestamp>()
            );
            // # Safety
            // compiler doesn't understand that the generic T here is actually Timestamp here since
            // we are using TUNNELED_TIMESTAMP_TYPE_NAME flag here which indicates a timestamp value
            // The assert statement above that compares the sizes of the Timestamp and value types
            let timestamp = unsafe { std::mem::transmute_copy::<&T, &Timestamp>(&value) };
            self.value_writer.with_annotations(self.annotations)?.write_timestamp(timestamp)
        } else if name == TUNNELED_DECIMAL_TYPE_NAME {
            // # Safety
            // compiler doesn't understand that the generic T here is actually Decimal here since
            // we are using TUNNELED_DECIMAL_TYPE_NAME flag here which indicates a decimal value
            // The assert statement above that compares the sizes of the Decimal and value types
            assert_eq!(std::mem::size_of_val(value), std::mem::size_of::<Decimal>());
            let decimal = unsafe { std::mem::transmute_copy::<&T, &Decimal>(&value) };
            self.value_writer.write_decimal(decimal)
        } else {
            value.serialize(self)
        }
    }

    fn serialize_newtype_variant<T>(
        mut self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.annotations.push(variant);
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(SeqWriter {
            seq_writer: self.value_writer.list_writer()?,
            is_human_readable: self.is_human_readable,
        })
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        let writer= self.value_writer.with_annotations(self.annotations)?;
        Ok(SeqWriter {
            seq_writer: writer.list_writer()?,
            is_human_readable: self.is_human_readable,
        })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        let ValueSerializer { value_writer, is_human_readable, mut annotations, .. } = self;
        annotations.push(name);
        Ok(SeqWriter {
            seq_writer: value_writer
                .with_annotations(annotations)?
                .list_writer()?,
            is_human_readable,
        })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        let ValueSerializer { value_writer, is_human_readable, mut annotations, .. } = self;
        annotations.push(variant);
        Ok(SeqWriter {
            seq_writer: value_writer
                .with_annotations(annotations)?
                .list_writer()?,
            is_human_readable,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(MapWriter {
            map_writer: self.value_writer.struct_writer()?,
            is_human_readable: self.is_human_readable,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(MapWriter {
            map_writer: self.value_writer.struct_writer()?,
            is_human_readable: self.is_human_readable,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        let ValueSerializer { value_writer, is_human_readable, mut annotations, .. } = self;
        annotations.push(variant);
        Ok(MapWriter {
            map_writer: value_writer
                .with_annotations(annotations)?
                .struct_writer()?,
            is_human_readable,
        })
    }
}

pub struct SeqWriter<V: ValueWriter> {
    seq_writer: V::ListWriter,
    is_human_readable: bool,
}

impl<V: ValueWriter> Deref for SeqWriter<V> {
    type Target = V::ListWriter;

    fn deref(&self) -> &Self::Target {
        &self.seq_writer
    }
}

impl<V: ValueWriter> DerefMut for SeqWriter<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.seq_writer
    }
}

impl<V: ValueWriter> ser::SerializeSeq for SeqWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        value.serialize(ValueSerializer::new(self.value_writer(), is_human_readable))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_writer.close()
    }
}

impl<V: ValueWriter> ser::SerializeTuple for SeqWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        value.serialize(ValueSerializer::new(self.value_writer(), is_human_readable))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_writer.close()
    }
}

impl<V: ValueWriter> ser::SerializeTupleStruct for SeqWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        value.serialize(ValueSerializer::new(self.value_writer(), is_human_readable))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_writer.close()
    }
}

impl<V: ValueWriter> ser::SerializeTupleVariant for SeqWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        value.serialize(ValueSerializer::new(self.value_writer(), is_human_readable))
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_writer.close()
    }
}

pub struct MapWriter<V: ValueWriter> {
    map_writer: V::StructWriter,
    is_human_readable: bool,
}

impl<V: ValueWriter> Deref for MapWriter<V> {
    type Target = V::StructWriter;

    fn deref(&self) -> &Self::Target {
        &self.map_writer
    }
}

impl<V: ValueWriter> DerefMut for MapWriter<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map_writer
    }
}

impl<V: ValueWriter> ser::SerializeMap for MapWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        // We need to verify that the key is a string type or can be converted
        // to string
        let mk_serializer = MapKeySerializer {};
        let field_name: String = key.serialize(mk_serializer)?;
        self.encode_field_name(field_name.as_str())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        let serializer = ValueSerializer::new(self.make_value_writer(), is_human_readable);
        value.serialize(serializer)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.map_writer.close()
    }
}

impl<V: ValueWriter> ser::SerializeStructVariant for MapWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        let serializer = ValueSerializer::new(self.field_writer(key), is_human_readable);
        value.serialize(serializer)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.map_writer.close()
    }
}

impl<V: ValueWriter> ser::SerializeStruct for MapWriter<V> {
    type Ok = ();
    type Error = IonError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        let is_human_readable = self.is_human_readable;
        let serializer = ValueSerializer::new(self.field_writer(key), is_human_readable);
        value.serialize(serializer)
    }

    fn end(self) -> Result<(), IonError> {
        self.map_writer.close()
    }
}

/// This serializer is utilized for handling maps with ion. Ion
/// does not support non-string keys for maps. However, we can support
/// other key types as long as the key type implements to_string.
struct MapKeySerializer {}

fn key_must_be_a_string() -> IonError {
    IonError::encoding_error("Ion does not support non-string keys for maps")
}

impl ser::Serializer for MapKeySerializer {
    // TODO: Adding a lifetime to MapKeySerializer would allow this to be Cow<'a, str> and avoid
    //       allocating in some cases.
    type Ok = String;
    type Error = IonError;

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(variant.to_string())
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    type SerializeSeq = Impossible<String, IonError>;
    type SerializeTuple = Impossible<String, IonError>;
    type SerializeTupleStruct = Impossible<String, IonError>;
    type SerializeTupleVariant = Impossible<String, IonError>;
    type SerializeMap = Impossible<String, IonError>;
    type SerializeStruct = Impossible<String, IonError>;
    type SerializeStructVariant = Impossible<String, IonError>;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(key_must_be_a_string())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(key_must_be_a_string())
    }
}
