//! Extração segura de ícones embutidos em executáveis Windows (PE).
//!
//! O parser lê apenas a tabela de recursos do arquivo. O executável nunca é
//! iniciado e nenhum utilitário externo é chamado.

use std::{
    fs::{self, File},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

const DOS_SIGNATURE: &[u8; 2] = b"MZ";
const PE_SIGNATURE: &[u8; 4] = b"PE\0\0";
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
// O parser atual mantém o PE na memória para validar todos os offsets. Limitar
// o tamanho impede que um executável malicioso ou incorreto esgote a memória.
const MAX_PE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 16_384;

#[derive(Clone, Copy, Debug)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

#[derive(Clone, Copy, Debug)]
struct ResourceData {
    id: Option<u32>,
    rva: u32,
    size: u32,
}

/// Gera um PNG em `<data_dir>/cache/icons` para um `.exe`/`.ico` local.
/// Retorna `None` quando o arquivo não possui um ícone legível.
pub fn cached_executable_icon(executable: &Path, data_dir: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(executable).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PE_BYTES {
        return None;
    }
    let extension = executable.extension()?.to_str()?.to_ascii_lowercase();
    if extension != "exe" && extension != "ico" {
        return None;
    }

    let mut identity = Sha256::new();
    identity.update(executable.to_string_lossy().as_bytes());
    identity.update(metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified() {
        if let Ok(value) = modified.duration_since(std::time::UNIX_EPOCH) {
            identity.update(value.as_nanos().to_le_bytes());
        }
    }
    let filename = format!("{:x}.png", identity.finalize());
    let directory = data_dir.join("cache/icons");
    let destination = directory.join(filename);
    if destination.is_file() {
        return Some(destination);
    }

    let icon = if extension == "ico" {
        ico::IconDir::read(File::open(executable).ok()?).ok()?
    } else {
        let mut bytes = Vec::with_capacity(metadata.len().min(32 * 1024 * 1024) as usize);
        File::open(executable).ok()?.read_to_end(&mut bytes).ok()?;
        extract_pe_icon(&bytes)?
    };
    let image = icon
        .entries()
        .iter()
        .filter_map(|entry| entry.decode().ok().map(|image| (entry, image)))
        .max_by_key(|(entry, _)| {
            (
                entry.width() as u64 * entry.height() as u64,
                entry.bits_per_pixel(),
            )
        })?
        .1;

    fs::create_dir_all(&directory).ok()?;
    let temporary = destination.with_extension(format!("png.{}.part", std::process::id()));
    image.write_png(File::create(&temporary).ok()?).ok()?;
    if fs::rename(&temporary, &destination).is_err() {
        let _ = fs::remove_file(&temporary);
        if !destination.is_file() {
            return None;
        }
    }
    Some(destination)
}

fn extract_pe_icon(bytes: &[u8]) -> Option<ico::IconDir> {
    if bytes.get(..2)? != DOS_SIGNATURE {
        return None;
    }
    let pe_offset = read_u32(bytes, 0x3c)? as usize;
    if bytes.get(pe_offset..pe_offset.checked_add(4)?)? != PE_SIGNATURE {
        return None;
    }
    let coff = pe_offset.checked_add(4)?;
    let section_count = read_u16(bytes, coff.checked_add(2)?)? as usize;
    let optional_size = read_u16(bytes, coff.checked_add(16)?)? as usize;
    if section_count == 0 || section_count > 96 {
        return None;
    }
    let optional = coff.checked_add(20)?;
    let magic = read_u16(bytes, optional)?;
    let data_directories = match magic {
        0x10b => optional.checked_add(96)?,
        0x20b => optional.checked_add(112)?,
        _ => return None,
    };
    let resource_directory = data_directories.checked_add(8 * 2)?;
    let resource_rva = read_u32(bytes, resource_directory)?;
    let resource_size = read_u32(bytes, resource_directory.checked_add(4)?)?;
    if resource_rva == 0 || resource_size == 0 || resource_size as usize > MAX_RESOURCE_BYTES {
        return None;
    }

    let section_table = optional.checked_add(optional_size)?;
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let offset = section_table.checked_add(index.checked_mul(40)?)?;
        sections.push(Section {
            virtual_size: read_u32(bytes, offset.checked_add(8)?)?,
            virtual_address: read_u32(bytes, offset.checked_add(12)?)?,
            raw_size: read_u32(bytes, offset.checked_add(16)?)?,
            raw_offset: read_u32(bytes, offset.checked_add(20)?)?,
        });
    }
    let resource_base = rva_to_offset(resource_rva, &sections, bytes.len())?;
    let groups = collect_resource_data(bytes, resource_base, RT_GROUP_ICON)?;
    let icons = collect_resource_data(bytes, resource_base, RT_ICON)?;
    let group = groups.first()?;
    let group_offset = rva_to_offset(group.rva, &sections, bytes.len())?;
    let group_bytes = bounded_slice(bytes, group_offset, group.size as usize)?;
    build_ico(group_bytes, &icons, bytes, &sections)
}

fn collect_resource_data(
    bytes: &[u8],
    base: usize,
    resource_type: u32,
) -> Option<Vec<ResourceData>> {
    let type_directory = find_directory(bytes, base, base, resource_type)?;
    let mut leaves = Vec::new();
    collect_directory_leaves(bytes, base, type_directory, 0, None, &mut leaves)?;
    (!leaves.is_empty()).then_some(leaves)
}

fn find_directory(bytes: &[u8], base: usize, directory: usize, id: u32) -> Option<usize> {
    let count = directory_count(bytes, directory)?;
    for index in 0..count {
        let entry = directory
            .checked_add(16)?
            .checked_add(index.checked_mul(8)?)?;
        let name = read_u32(bytes, entry)?;
        let target = read_u32(bytes, entry.checked_add(4)?)?;
        if name & 0x8000_0000 == 0 && name == id && target & 0x8000_0000 != 0 {
            return base.checked_add((target & 0x7fff_ffff) as usize);
        }
    }
    None
}

fn collect_directory_leaves(
    bytes: &[u8],
    base: usize,
    directory: usize,
    depth: usize,
    inherited_id: Option<u32>,
    output: &mut Vec<ResourceData>,
) -> Option<()> {
    if depth > 4 || output.len() >= MAX_DIRECTORY_ENTRIES {
        return None;
    }
    let count = directory_count(bytes, directory)?;
    for index in 0..count {
        let entry = directory
            .checked_add(16)?
            .checked_add(index.checked_mul(8)?)?;
        let name = read_u32(bytes, entry)?;
        let target = read_u32(bytes, entry.checked_add(4)?)?;
        // O identificador útil é o primeiro nível abaixo de RT_ICON. O nível
        // seguinte normalmente é o idioma (por exemplo 1033) e não pode
        // substituir o ID usado por RT_GROUP_ICON para referenciar a imagem.
        let id = inherited_id.or_else(|| (name & 0x8000_0000 == 0).then_some(name));
        let target_offset = base.checked_add((target & 0x7fff_ffff) as usize)?;
        if target & 0x8000_0000 != 0 {
            collect_directory_leaves(bytes, base, target_offset, depth + 1, id, output)?;
        } else {
            output.push(ResourceData {
                id,
                rva: read_u32(bytes, target_offset)?,
                size: read_u32(bytes, target_offset.checked_add(4)?)?,
            });
        }
    }
    Some(())
}

fn directory_count(bytes: &[u8], directory: usize) -> Option<usize> {
    let named = read_u16(bytes, directory.checked_add(12)?)? as usize;
    let ids = read_u16(bytes, directory.checked_add(14)?)? as usize;
    let count = named.checked_add(ids)?;
    if count > MAX_DIRECTORY_ENTRIES {
        return None;
    }
    bounded_slice(bytes, directory.checked_add(16)?, count.checked_mul(8)?)?;
    Some(count)
}

fn build_ico(
    group: &[u8],
    icons: &[ResourceData],
    image: &[u8],
    sections: &[Section],
) -> Option<ico::IconDir> {
    if read_u16(group, 0)? != 0 || read_u16(group, 2)? != 1 {
        return None;
    }
    let count = read_u16(group, 4)? as usize;
    if count == 0 || count > 256 {
        return None;
    }
    bounded_slice(group, 6, count.checked_mul(14)?)?;
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&[0, 0, 1, 0]);
    encoded.extend_from_slice(&(count as u16).to_le_bytes());
    let table_size = 6usize.checked_add(count.checked_mul(16)?)?;
    let mut payload = Vec::new();
    for index in 0..count {
        let source = 6usize.checked_add(index.checked_mul(14)?)?;
        let icon_id = read_u16(group, source.checked_add(12)?)? as u32;
        let resource = icons.iter().find(|icon| icon.id == Some(icon_id))?;
        let offset = rva_to_offset(resource.rva, sections, image.len())?;
        let data = bounded_slice(image, offset, resource.size as usize)?;
        if data.len() > MAX_RESOURCE_BYTES {
            return None;
        }
        encoded.extend_from_slice(group.get(source..source.checked_add(8)?)?);
        encoded.extend_from_slice(&(data.len() as u32).to_le_bytes());
        encoded.extend_from_slice(&(table_size.checked_add(payload.len())? as u32).to_le_bytes());
        payload.extend_from_slice(data);
    }
    encoded.extend_from_slice(&payload);
    ico::IconDir::read(Cursor::new(encoded)).ok()
}

fn rva_to_offset(rva: u32, sections: &[Section], image_len: usize) -> Option<usize> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_size);
        if rva >= section.virtual_address && rva < section.virtual_address.checked_add(span)? {
            let relative = rva.checked_sub(section.virtual_address)?;
            if relative >= section.raw_size {
                return None;
            }
            let offset = section.raw_offset.checked_add(relative)? as usize;
            return (offset < image_len).then_some(offset);
        }
    }
    None
}

fn bounded_slice(bytes: &[u8], offset: usize, length: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(length)?)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bounded_slice(bytes, offset, 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bounded_slice(bytes, offset, 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_pe_data_without_panicking() {
        assert!(extract_pe_icon(b"not an executable").is_none());
        let malformed = [b'M', b'Z', 0, 0];
        assert!(extract_pe_icon(&malformed).is_none());
    }

    #[test]
    fn checks_all_integer_bounds_on_malformed_pe() {
        let mut malformed = vec![0_u8; 128];
        malformed[..2].copy_from_slice(DOS_SIGNATURE);
        malformed[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(extract_pe_icon(&malformed).is_none());
    }

    #[test]
    fn icon_cache_ignores_unsupported_files() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("example.txt");
        fs::write(&executable, b"MZ").unwrap();
        assert!(cached_executable_icon(&executable, directory.path()).is_none());
    }

    #[test]
    fn extracts_an_external_fixture_when_supplied() {
        let Some(fixture) = std::env::var_os("ORBIT_PE_ICON_FIXTURE") else {
            return;
        };
        let directory = tempfile::tempdir().unwrap();
        let icon = cached_executable_icon(Path::new(&fixture), directory.path())
            .expect("o fixture PE deveria conter um ícone");
        assert!(icon.is_file());
        ico::IconImage::read_png(File::open(icon).unwrap()).unwrap();
    }
}
