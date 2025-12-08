#!/bin/bash
# NexaOS Build System - YAML Configuration Parser
# Provides functions to parse build-config.yaml
#
# This is a lightweight YAML parser for bash that handles the specific
# structure of build-config.yaml without requiring external dependencies.

# ============================================================================
# YAML Parsing Functions
# ============================================================================

# Get the path to build-config.yaml
get_config_path() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    # config-parser.sh is in scripts/lib/, config is in scripts/
    echo "$(dirname "$script_dir")/build-config.yaml"
}

# Parse programs from YAML and populate PROGRAMS array
# Output format: "package:binary:dest:features:link"
parse_programs_config() {
    local config_file
    config_file="$(get_config_path)"
    
    if [ ! -f "$config_file" ]; then
        echo "Error: build-config.yaml not found at $config_file" >&2
        return 1
    fi
    
    local in_programs=0
    local in_category=0
    local in_item=0
    local package="" binary="" dest="" features="" link=""
    
    while IFS= read -r line || [ -n "$line" ]; do
        # Skip comments and empty lines
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ -z "${line// }" ]] && continue
        
        # Detect programs section
        if [[ "$line" =~ ^programs: ]]; then
            in_programs=1
            continue
        fi
        
        # Exit programs section when we hit another top-level key
        if [[ "$line" =~ ^[a-z_]+: ]] && [[ ! "$line" =~ ^[[:space:]] ]] && [ $in_programs -eq 1 ]; then
            # Output last item if pending
            if [ -n "$package" ]; then
                [ -z "$binary" ] && binary="$package"
                [ -z "$link" ] && link="dyn"
                echo "${package}:${binary}:${dest}:${features}:${link}"
                package=""  # Clear to prevent duplicate output after loop
            fi
            break
        fi
        
        [ $in_programs -eq 0 ] && continue
        
        # Detect category (e.g., "  core:")
        if [[ "$line" =~ ^[[:space:]]{2}[a-z_]+: ]]; then
            # Output previous item if exists
            if [ -n "$package" ]; then
                [ -z "$binary" ] && binary="$package"
                [ -z "$link" ] && link="dyn"
                echo "${package}:${binary}:${dest}:${features}:${link}"
                package="" binary="" dest="" features="" link=""
            fi
            in_category=1
            continue
        fi
        
        # Detect list item start (e.g., "    - package:")
        if [[ "$line" =~ ^[[:space:]]{4}-[[:space:]] ]]; then
            # Output previous item if exists
            if [ -n "$package" ]; then
                [ -z "$binary" ] && binary="$package"
                [ -z "$link" ] && link="dyn"
                echo "${package}:${binary}:${dest}:${features}:${link}"
            fi
            package="" binary="" dest="" features="" link=""
            in_item=1
            
            # Handle inline "- package: value" format
            if [[ "$line" =~ -[[:space:]]+package:[[:space:]]*(.+) ]]; then
                package="${BASH_REMATCH[1]}"
            fi
            continue
        fi
        
        # Parse item properties
        if [ $in_item -eq 1 ]; then
            if [[ "$line" =~ ^[[:space:]]+package:[[:space:]]*(.+) ]]; then
                package="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+binary:[[:space:]]*(.+) ]]; then
                binary="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+dest:[[:space:]]*(.+) ]]; then
                dest="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+features:[[:space:]]*(.+) ]]; then
                features="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+link:[[:space:]]*(.+) ]]; then
                link="${BASH_REMATCH[1]}"
            fi
        fi
        
    done < "$config_file"
    
    # Output last item if pending (in case file doesn't end with newline)
    if [ -n "$package" ]; then
        [ -z "$binary" ] && binary="$package"
        [ -z "$link" ] && link="dyn"
        echo "${package}:${binary}:${dest}:${features}:${link}"
    fi
}

# Parse modules from YAML
# Output format: "name:type:description"
parse_modules_config() {
    local config_file
    config_file="$(get_config_path)"
    
    if [ ! -f "$config_file" ]; then
        echo "Error: build-config.yaml not found at $config_file" >&2
        return 1
    fi
    
    local in_modules=0
    local in_category=0
    local in_item=0
    local name="" type="" description=""
    
    while IFS= read -r line || [ -n "$line" ]; do
        # Skip comments and empty lines
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ -z "${line// }" ]] && continue
        
        # Detect modules section
        if [[ "$line" =~ ^modules: ]]; then
            in_modules=1
            continue
        fi
        
        # Exit modules section when we hit another top-level key
        if [[ "$line" =~ ^[a-z_]+: ]] && [[ ! "$line" =~ ^[[:space:]] ]] && [ $in_modules -eq 1 ]; then
            # Output last item if pending
            if [ -n "$name" ]; then
                echo "${name}:${type}:${description}"
                name=""  # Clear to prevent duplicate output after loop
            fi
            break
        fi
        
        [ $in_modules -eq 0 ] && continue
        
        # Detect category (e.g., "  filesystem:")
        if [[ "$line" =~ ^[[:space:]]{2}[a-z_]+: ]]; then
            # Output previous item if exists
            if [ -n "$name" ]; then
                echo "${name}:${type}:${description}"
                name="" type="" description=""
            fi
            in_category=1
            continue
        fi
        
        # Detect list item start
        if [[ "$line" =~ ^[[:space:]]{4}-[[:space:]] ]]; then
            # Output previous item if exists
            if [ -n "$name" ]; then
                echo "${name}:${type}:${description}"
            fi
            name="" type="" description=""
            in_item=1
            
            # Handle inline format
            if [[ "$line" =~ -[[:space:]]+name:[[:space:]]*(.+) ]]; then
                name="${BASH_REMATCH[1]}"
            fi
            continue
        fi
        
        # Parse item properties
        if [ $in_item -eq 1 ]; then
            if [[ "$line" =~ ^[[:space:]]+name:[[:space:]]*(.+) ]]; then
                name="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+type:[[:space:]]*([0-9]+) ]]; then
                type="${BASH_REMATCH[1]}"
            elif [[ "$line" =~ ^[[:space:]]+description:[[:space:]]*[\"\']*([^\"\']+)[\"\']*$ ]]; then
                description="${BASH_REMATCH[1]}"
            fi
        fi
        
    done < "$config_file"
    
    # Output last item if pending
    if [ -n "$name" ]; then
        echo "${name}:${type}:${description}"
    fi
}

# Parse libraries from YAML
# Output format: "name:output:version:depends"
parse_libraries_config() {
    local config_file
    config_file="$(get_config_path)"
    
    if [ ! -f "$config_file" ]; then
        echo "Error: build-config.yaml not found at $config_file" >&2
        return 1
    fi
    
    local in_libraries=0
    local in_item=0
    local in_depends=0
    local name="" output="" version="" depends=""
    
    while IFS= read -r line || [ -n "$line" ]; do
        # Skip comments and empty lines
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ -z "${line// }" ]] && continue
        
        # Detect libraries section
        if [[ "$line" =~ ^libraries: ]]; then
            in_libraries=1
            continue
        fi
        
        # Exit libraries section when we hit another top-level key
        if [[ "$line" =~ ^[a-z_]+: ]] && [[ ! "$line" =~ ^[[:space:]] ]] && [ $in_libraries -eq 1 ]; then
            # Output last item if pending
            if [ -n "$name" ]; then
                echo "${name}:${output}:${version}:${depends}"
                name=""  # Clear to prevent duplicate output after loop
            fi
            break
        fi
        
        [ $in_libraries -eq 0 ] && continue
        
        # Detect list item start (e.g., "  - name:")
        if [[ "$line" =~ ^[[:space:]]{2}-[[:space:]] ]]; then
            # Output previous item if exists
            if [ -n "$name" ]; then
                echo "${name}:${output}:${version}:${depends}"
            fi
            name="" output="" version="" depends=""
            in_item=1
            in_depends=0
            
            # Handle inline format
            if [[ "$line" =~ -[[:space:]]+name:[[:space:]]*(.+) ]]; then
                name="${BASH_REMATCH[1]}"
            fi
            continue
        fi
        
        # Parse item properties
        if [ $in_item -eq 1 ]; then
            if [[ "$line" =~ ^[[:space:]]+name:[[:space:]]*(.+) ]]; then
                name="${BASH_REMATCH[1]}"
                in_depends=0
            elif [[ "$line" =~ ^[[:space:]]+output:[[:space:]]*(.+) ]]; then
                output="${BASH_REMATCH[1]}"
                in_depends=0
            elif [[ "$line" =~ ^[[:space:]]+version:[[:space:]]*([0-9]+) ]]; then
                version="${BASH_REMATCH[1]}"
                in_depends=0
            elif [[ "$line" =~ ^[[:space:]]+depends:[[:space:]]*\[\] ]]; then
                # Empty depends array
                depends=""
                in_depends=0
            elif [[ "$line" =~ ^[[:space:]]+depends: ]]; then
                in_depends=1
            elif [ $in_depends -eq 1 ] && [[ "$line" =~ ^[[:space:]]+-[[:space:]]*(.+) ]]; then
                # Dependency item
                local dep="${BASH_REMATCH[1]}"
                if [ -z "$depends" ]; then
                    depends="$dep"
                else
                    depends="$depends,$dep"
                fi
            elif [[ "$line" =~ ^[[:space:]]+[a-z] ]] && [ $in_depends -eq 1 ]; then
                # New property, end depends parsing
                in_depends=0
            fi
        fi
        
    done < "$config_file"
    
    # Output last item if pending
    if [ -n "$name" ]; then
        echo "${name}:${output}:${version}:${depends}"
    fi
}

# Load programs into PROGRAMS array
load_programs_array() {
    PROGRAMS=()
    while IFS= read -r line; do
        [ -n "$line" ] && PROGRAMS+=("$line")
    done < <(parse_programs_config)
}

# Load modules into MODULES array
load_modules_array() {
    MODULES=()
    while IFS= read -r line; do
        [ -n "$line" ] && MODULES+=("$line")
    done < <(parse_modules_config)
}

# Get library build order
get_library_build_order() {
    local config_file
    config_file="$(get_config_path)"
    
    local in_build_order=0
    local in_libraries=0
    
    while IFS= read -r line || [ -n "$line" ]; do
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ -z "${line// }" ]] && continue
        
        if [[ "$line" =~ ^build_order: ]]; then
            in_build_order=1
            continue
        fi
        
        [ $in_build_order -eq 0 ] && continue
        
        if [[ "$line" =~ ^[[:space:]]{2}libraries: ]]; then
            in_libraries=1
            continue
        fi
        
        # Exit on next section
        if [[ "$line" =~ ^[a-z_]+: ]] && [[ ! "$line" =~ ^[[:space:]] ]]; then
            break
        fi
        
        if [ $in_libraries -eq 1 ] && [[ "$line" =~ ^[[:space:]]+-[[:space:]]*(.+) ]]; then
            echo "${BASH_REMATCH[1]}"
        fi
        
    done < "$config_file"
}

# ============================================================================
# Helper functions for scripts
# ============================================================================

# List all program packages
list_all_programs() {
    parse_programs_config | while IFS=':' read -r package binary dest features link; do
        echo "$package -> $binary (/$dest) [$link]"
    done
}

# List all modules
list_all_modules() {
    parse_modules_config | while IFS=':' read -r name type desc; do
        echo "$name (type $type): $desc"
    done
}

# List all libraries
list_all_libraries() {
    parse_libraries_config | while IFS=':' read -r name output version depends; do
        local deps_str=""
        [ -n "$depends" ] && deps_str=" (depends: $depends)"
        echo "lib${output}.so.${version} ($name)$deps_str"
    done
}

# Find a program by package name
find_program() {
    local search="$1"
    parse_programs_config | while IFS=':' read -r package binary dest features link; do
        if [ "$package" = "$search" ] || [ "$binary" = "$search" ]; then
            echo "${package}:${binary}:${dest}:${features}:${link}"
            return 0
        fi
    done
}

# ============================================================================
# Self-test
# ============================================================================

if [ "${BASH_SOURCE[0]}" == "${0}" ]; then
    case "${1:-test}" in
        programs)
            echo "Parsed programs:"
            parse_programs_config
            ;;
        modules)
            echo "Parsed modules:"
            parse_modules_config
            ;;
        libraries)
            echo "Parsed libraries:"
            parse_libraries_config
            ;;
        build-order)
            echo "Library build order:"
            get_library_build_order
            ;;
        list)
            echo "=== Programs ==="
            list_all_programs
            echo ""
            echo "=== Modules ==="
            list_all_modules
            echo ""
            echo "=== Libraries ==="
            list_all_libraries
            ;;
        test)
            echo "Testing YAML parser..."
            echo ""
            echo "=== Programs ($(parse_programs_config | wc -l) items) ==="
            parse_programs_config | head -5
            echo "..."
            echo ""
            echo "=== Modules ($(parse_modules_config | wc -l) items) ==="
            parse_modules_config
            echo ""
            echo "=== Libraries ($(parse_libraries_config | wc -l) items) ==="
            parse_libraries_config
            echo ""
            echo "=== Library Build Order ==="
            get_library_build_order
            echo ""
            echo "Test complete!"
            ;;
        *)
            echo "Usage: $0 {programs|modules|libraries|build-order|list|test}"
            ;;
    esac
fi
