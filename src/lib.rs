#![feature(old_io)]
#![feature(core)]

extern crate num;

use std::old_io;
use std::rc::Rc;
use std::borrow::ToOwned;
use std::old_io::net::tcp;
use std::intrinsics::transmute;

//use core::old_io::{ReaderUtil, WriterUtil};

//use std::{net_ip, net_tcp, uv_global_loop, Bigint};
//use std::old_io_util;

pub static CQL_VERSION:u8 = 0x01;

#[derive(Clone, Debug)]
enum OpcodeReq {
    //requests
    Startup = 0x01,
    Cred = 0x04,
    Opts = 0x05,
    Query = 0x07,
    Prepare = 0x09,
    Register = 0x0B,
}

#[derive(Debug)]
enum OpcodeResp {
    //responces
    Error = 0x00,
    Ready = 0x02,
    Auth = 0x03,
    Supported = 0x06,
    Result = 0x08,
    Exec = 0x0A,
    Event = 0x0C,
}

fn opcode(val: u8) -> OpcodeResp {
    match val {
//        0x01 => Startup,
//        0x04 => Cred,
//        0x05 => Opts,
//        0x07 => Query,
//        0x09 => Prepare,
//        0x0B => Register,

        0x00 => OpcodeResp::Error,
        0x02 => OpcodeResp::Ready,
        0x03 => OpcodeResp::Auth,
        0x06 => OpcodeResp::Supported,
        0x08 => OpcodeResp::Result,
        0x0A => OpcodeResp::Exec,
        0x0C => OpcodeResp::Event,
        _ => OpcodeResp::Error
    }
}

#[derive(Clone, Debug)]
pub enum Consistency {
    Any = 0x0000,
    One = 0x0001,
    Two = 0x0002,
    Three = 0x0003,
    Quorum = 0x0004,
    All = 0x0005,
    LocalQuorum = 0x0006,
    EachQuorum = 0x0007,
    Unknown,
}

pub fn consistency(val: u16) -> Consistency {
    match val {
        0 => Consistency::Any,
        1 => Consistency::One,
        2 => Consistency::Two,
        3 => Consistency::Three,
        4 => Consistency::Quorum,
        5 => Consistency::All,
        6 => Consistency::LocalQuorum,
        7 => Consistency::EachQuorum,
        _ => Consistency::Unknown
    }
}

#[derive(Clone, Debug)]
pub enum ColumnType {
    Custom = 0x0000,
    Ascii = 0x0001,
    Bigint = 0x0002,
    Blob = 0x0003,
    Boolean = 0x0004,
    Counter = 0x0005,
    Decimal = 0x0006,
    Double = 0x0007,
    Float = 0x0008,
    Int = 0x0009,
    Text = 0x000A,
    Timestamp = 0x000B,
    UUID = 0x000C,
    VarChar = 0x000D,
    Varint = 0x000E,
    TimeUUID = 0x000F,
    Inet = 0x0010,
    List = 0x0020,
    Map = 0x0021,
    Set = 0x0022,
    Unknown = 0xffff,
}

fn column_type(val: u16) -> ColumnType {
    match val {
        0x0000 => ColumnType::Custom,
        0x0001 => ColumnType::Ascii,
        0x0002 => ColumnType::Bigint,
        0x0003 => ColumnType::Blob,
        0x0004 => ColumnType::Boolean,
        0x0005 => ColumnType::Counter,
        0x0006 => ColumnType::Decimal,
        0x0007 => ColumnType::Double,
        0x0008 => ColumnType::Float,
        0x0009 => ColumnType::Int,
        0x000A => ColumnType::Text,
        0x000B => ColumnType::Timestamp,
        0x000C => ColumnType::UUID,
        0x000D => ColumnType::VarChar,
        0x000E => ColumnType::Varint,
        0x000F => ColumnType::TimeUUID,
        0x0010 => ColumnType::Inet,
        0x0020 => ColumnType::List,
        0x0021 => ColumnType::Map,
        0x0022 => ColumnType::Set,
        _ => ColumnType::Unknown
    }
}

#[derive(Debug)]
pub struct Error {
    name: String,
    msg: String,
}

trait CqlSerializable {
    fn len(&self) -> usize;
    fn serialize<T: old_io::Writer>(&self, buf: &mut T);
}

trait CqlReader {
    fn read_bytes(&mut self, len: usize) -> Vec<u8>;
    fn read_cql_str(&mut self) -> String;
    fn read_cql_long_str(&mut self) -> Option<String>;
    fn read_cql_rows(&mut self) -> Rows;

    fn read_cql_metadata(&mut self) -> Metadata;
    fn read_cql_response(&mut self) -> Response;
}

impl<'a, T: old_io::Reader> CqlReader for T {
    fn read_bytes(&mut self, len: usize) -> Vec<u8> {
        self.read_exact(len).unwrap()
    }

    fn read_cql_str(&mut self) -> String {
        let len = self.read_be_u16().unwrap() as usize;
        String::from_utf8(self.read_bytes(len)).unwrap()
    }

    fn read_cql_long_str(&mut self) -> Option<String> {
        match self.read_be_i32().unwrap() {
            -1 => None,
            len => Some(String::from_utf8(self.read_bytes(len as usize)).unwrap())
        }
    }

    fn read_cql_metadata(&mut self) -> Metadata {
        let flags = self.read_be_u32().unwrap();
        let column_count = self.read_be_u32().unwrap();
        let (keyspace, table) =
            if flags == 0x0001 {
                let keyspace_str = self.read_cql_str();
                let table_str = self.read_cql_str();
                (Some(keyspace_str), Some(table_str))
            } else {
                (None, None)
            };

        let mut row_metadata:Vec<CqlColMetadata> = Vec::new();
        for _ in (0 .. column_count) {
            let (keyspace, table) =
                if flags == 0x0001 {
                    (None, None)
                } else {
                    let keyspace_str = self.read_cql_str();
                    let table_str = self.read_cql_str();
                    (Some(keyspace_str), Some(table_str))
                };
            let col_name = self.read_cql_str();
            let type_key = self.read_be_u16().unwrap();
            let type_name =
                if type_key >= 0x20 {
                    column_type(self.read_be_u16().unwrap())
                } else {
                    ColumnType::Unknown
                };

            row_metadata.push(CqlColMetadata {
                keyspace: keyspace,
                table: table,
                col_name: col_name,
                col_type: column_type(type_key),
                col_type_name: type_name
            });
        }

        Metadata {
            flags: flags,
            column_count: column_count,
            keyspace: keyspace,
            table: table,
            row_metadata: row_metadata,
        }
    }

    fn read_cql_rows(&mut self) -> Rows {
        let metadata = Rc::new(self.read_cql_metadata());
        let rows_count = self.read_be_u32().unwrap();

        let mut rows:Vec<Row> = Vec::new();
        for _ in (0 .. rows_count) {
            let mut row = Row{ cols: Vec::new(), metadata: metadata.clone() };
            for meta in row.metadata.row_metadata.iter() {
                let col = match meta.col_type.clone() {
                    ColumnType::Ascii => Cql::CqlString(self.read_cql_long_str()),
                    ColumnType::VarChar => Cql::CqlString(self.read_cql_long_str()),
                    ColumnType::Text => Cql::CqlString(self.read_cql_long_str()),

                    ColumnType::Int => Cql::Cqli32(match self.read_be_i32().unwrap() {
                            -1 => None,
                            4 => Some(self.read_be_i32().unwrap()),
                            len => panic!("Invalid length with i32: {}", len),
                        }),
                    ColumnType::Bigint => Cql::Cqli64(Some(self.read_be_i64().unwrap())),
                    ColumnType::Float => Cql::Cqlf32(unsafe{
                        match self.read_be_i32().unwrap() {
                            -1 => None,
                            4 => Some(transmute(self.read_be_u32().unwrap())),
                            len => panic!("Invalid length with f32: {}", len),
                        }
                    }),
                    ColumnType::Double => Cql::Cqlf64(unsafe{
                        match self.read_be_i32().unwrap() {
                            -1 => None,
                            4 => Some(transmute(self.read_be_u64().unwrap())),
                            len => panic!("Invalid length with f64: {}", len),
                        }
                    }),

                    ColumnType::List => Cql::CqlList({
                        match self.read_be_i32().unwrap() {
                            -1 => None,
                            _ => {
                                //let data = self.read_bytes(len as usize);
                                panic!("List parse not implemented: {}");
                            },
                        }
                    }),


//                    Custom => ,
//                    Blob => ,
//                    Boolean => ,
//                    Counter => ,
//                    Decimal => ,
//                    Timestamp => ,
//                    UUID => ,
//                    Varint => ,
//                    TimeUUID => ,
//                    Inet => ,
//                    List => ,
//                    Map => ,
//                    Set => ,

                    _ => {
                        match self.read_be_i32().unwrap() {
                            -1 => (),
                            len => { self.read_bytes(len as usize); },
                        }
                        Cql::CqlUnknown
                    }
                };

                row.cols.push(col);
            }
            rows.push(row);
        }

        Rows {
            metadata: metadata,
            rows: rows,
        }
    }

    fn read_cql_response(&mut self) -> Response {
        let header_data = self.read_bytes(8);

        let version = header_data[0];
        let flags = header_data[1];
        let stream = header_data[2] as i8;
        let opcode = opcode(header_data[3]);
        let length = ((header_data[4] as u32) << 24u32) +
            ((header_data[5] as u32) << 16u32) +
            ((header_data[6] as u32) << 8u32) +
            (header_data[7] as u32);

        let body_data = self.read_bytes(length as usize);
        let mut reader = std::old_io::BufferedReader::new(body_data.as_slice());

        let body = match opcode {
            OpcodeResp::Ready => ResponseBody::Ready,
            OpcodeResp::Auth => {
                ResponseBody::Auth(reader.read_cql_str())
            }
            OpcodeResp::Error => {
                let code = reader.read_be_u32().unwrap();
                let msg = reader.read_cql_str();
                ResponseBody::Error(code, msg)
            },
            OpcodeResp::Result => {
                let code = reader.read_be_u32().unwrap();
                match code {
                    0x0001 => {
                        ResponseBody::Void
                    },
                    0x0002 => {
                        ResponseBody::Rows(reader.read_cql_rows())
                    },
                    0x0003 => {
                        let msg = reader.read_cql_str();
                        ResponseBody::Keyspace(msg)
                    },
                    0x0004 => {
                        let id = reader.read_u8().unwrap();
                        let metadata = reader.read_cql_metadata();
                        ResponseBody::Prepared(id, metadata)
                    },
                    0x0005 => {
                        let change  = reader.read_cql_str();
                        let keyspace = reader.read_cql_str();
                        let table = reader.read_cql_str();
                        ResponseBody::SchemaChange(change, keyspace, table)
                    },
                    _ => {
                        panic!("Unknown code for result: {}", code);
                    },
                }
            }
            _ => {
                panic!("Invalid response from server");
            },
        };

/*
        if reader.pos != length as usize {
            panic!("Data is not fully readed: specificatold_ion might be changed {} != {}",
                reader.pos, length);
        }
        */

        return Response {
            version: version,
            flags: flags,
            stream: stream,
            opcode: opcode,
            body: body,
        };
    }
}

#[derive(Debug)]
struct Pair {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl CqlSerializable for Pair {
    fn serialize<T: old_io::Writer>(&self, buf: &mut T) {
        buf.write_be_u16(self.key.len() as u16).unwrap();
        buf.write_all(self.key.as_slice()).unwrap();
        buf.write_be_u16(self.value.len() as u16).unwrap();
        buf.write_all(self.value.as_slice()).unwrap();
    }

    fn len(&self) -> usize {
        return 4 + self.key.len() + self.value.len();
    }
}

#[derive(Debug)]
pub struct StringMap {
    pairs: Vec<Pair>,
}

impl CqlSerializable for StringMap {
    fn serialize<T: old_io::Writer>(&self, buf: &mut T) {
        buf.write_be_u16(self.pairs.len() as u16).unwrap();
        for pair in self.pairs.iter() {
            pair.serialize(buf);
        }
    }

    fn len(&self) -> usize {
        let mut len = 2usize;
        for pair in self.pairs.iter() {
            len += pair.len();
        }
        len
    }
}

#[derive(Debug)]
struct CqlColMetadata {
    keyspace: Option<String>,
    table: Option<String>,
    col_name: String,
    col_type: ColumnType,
    col_type_name: ColumnType,
}

#[derive(Debug)]
pub struct Metadata {
    flags: u32,
    column_count: u32,
    keyspace: Option<String>,
    table: Option<String>,
    row_metadata: Vec<CqlColMetadata>,
}

#[derive(Clone, Debug)]
pub enum Cql {
    CqlString(Option<String>),

    Cqli32(Option<i32>),
    Cqli64(Option<i64>),

    CqlBlob(Option<Vec<u8>>),
    CqlBool(Option<bool>),

    CqlCounter(Option<u64>),

    Cqlf32(Option<f32>),
    Cqlf64(Option<f64>),

    CqlTimestamp(u64),
    CqlBigint(num::BigInt),

    CqlList(Option<Vec<Cql>>),

    CqlUnknown,
}

#[derive(Debug)]
pub struct Row {
    cols: Vec<Cql>,
    metadata: Rc<Metadata>,
}

impl Row {
    pub fn get_column(&self, col_name: &str) -> Option<Cql> {
        let mut i = 0;
        for metadata in self.metadata.row_metadata.iter() {
            if metadata.col_name == col_name {
                return Some(self.cols[i].clone());
            }
            i += 1;
        }
        None
    }
}

#[derive(Debug)]
pub struct Rows {
    metadata: Rc<Metadata>,
    rows: Vec<Row>,
}

#[derive(Debug)]
pub enum RequestBody {
    RequestStartup(StringMap),
    RequestCred(Vec<Vec<u8>>),
    RequestQuery(String, Consistency),
    RequestOptions,
}

#[derive(Debug)]
pub enum ResponseBody {
    Error(u32, String),
    Ready,
    Auth(String),

    Void,
    Rows(Rows),
    Keyspace(String),
    Prepared(u8, Metadata),
    SchemaChange(String, String, String),
}

#[derive(Debug)]
struct Request {
    version: u8,
    flags: u8,
    stream: i8,
    opcode: OpcodeReq,
    body: RequestBody,
}

#[derive(Debug)]
pub struct Response {
    version: u8,
    flags: u8,
    stream: i8,
    opcode: OpcodeResp,
    body: ResponseBody,
}

impl CqlSerializable for Request {
    fn serialize<T: old_io::Writer>(&self, buf: &mut T) {
        buf.write_u8(self.version).unwrap();
        buf.write_u8(self.flags).unwrap();
        buf.write_i8(self.stream).unwrap();
        buf.write_u8(self.opcode.clone() as u8).unwrap();
        buf.write_be_u32((self.len()-8) as u32).unwrap();

        match self.body {
            RequestBody::RequestStartup(ref map) => {
                map.serialize(buf)
            },
            RequestBody::RequestQuery(ref query_str, ref consistency) => {
                buf.write_be_u32(query_str.len() as u32).unwrap();
                buf.write_all(query_str.as_bytes()).unwrap();
                buf.write_be_u16(consistency.clone() as u16).unwrap();
            },
            _ => (),
        }
    }
    fn len(&self) -> usize {
        8 + match self.body {
            RequestBody::RequestStartup(ref map) => {
                map.len()
            },
            RequestBody::RequestQuery(ref query_str, _) => {
                4 + query_str.len() + 2
            },
            _ => {
                0
            }
        }
    }
}

fn startup() -> Request {
    let body = StringMap {
            pairs:vec![Pair{key: b"CQL_VERSION".to_owned(), value: b"3.0.0".to_owned()}],
        };
    return Request {
        version: CQL_VERSION,
        flags: 0x00,
        stream: 0x01,
        opcode: OpcodeReq::Startup,
        body: RequestBody::RequestStartup(body),
    };
}

fn auth(creds: Vec<Vec<u8>>) -> Request {
    return Request {
        version: CQL_VERSION,
        flags: 0x00,
        stream: 0x01,
        opcode: OpcodeReq::Cred,
        body: RequestBody::RequestCred(creds),
    };
}

fn options() -> Request {
    return Request {
        version: CQL_VERSION,
        flags: 0x00,
        stream: 0x01,
        opcode: OpcodeReq::Opts,
        body: RequestBody::RequestOptions,
    };
}

fn query(stream: i8, query_str: &str, con: Consistency) -> Request {
    return Request {
        version: CQL_VERSION,
        flags: 0x00,
        stream: stream,
        opcode: OpcodeReq::Query,
        body: RequestBody::RequestQuery(query_str.to_string(), con),
    };
}

pub struct Client {
    socket: tcp::TcpStream,
}

impl Client {
    pub fn query(&mut self, query_str: &str, con: Consistency) -> Response {
        let q = query(0, query_str, con);

        let mut writer = Vec::new();

        q.serialize::<Vec<u8>>(&mut writer);
        self.socket.write_all(writer.as_slice()).unwrap();
        self.socket.read_cql_response()
    }
}

pub fn connect(addr: &str) -> Result<Client, Error> {
    let res = tcp::TcpStream::connect(addr);
    if res.is_err() {
        return Err(Error{name: "Error".to_string(), msg: "Failed to connect to server".to_string()});
    }

    let mut socket = res.unwrap();

    let msg_startup = startup();
    msg_startup.serialize::<tcp::TcpStream>(&mut socket);

    let response = socket.read_cql_response();
    match response.body {
        ResponseBody::Ready => {
            Ok(Client { socket: socket })
        },
        /*
        Auth(_) => {
            match(creds) {
                Some(cred) => {
                    let msg_auth = Auth(cred);
                    msg_auth.serialize::<net_tcp::TcpSocketBuf>(&buf);
                    let response = buf.read_cql_response();
                    match response.body {
                        Ready => result::Ok(Client { socket: buf }),
                        Error(_, ref msg) => {
                            result::Err(Error(~"Error", copy *msg))
                        }
                        _ => {
                            result::Err(Error(~"Error", ~"Server returned unknown message"))
                        },
                    }
                },
                None => {
                    result::Err(Error(~"Error", ~"Credential should be provided"))
                },
            }

        }
        */
        _ => panic!("invalid opcode: {}", response.opcode as u8)
    }
}