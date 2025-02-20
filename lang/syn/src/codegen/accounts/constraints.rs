use crate::*;
use proc_macro2_diagnostics::SpanDiagnosticExt;
use quote::quote;
use syn::Expr;

pub fn generate(f: &Field) -> proc_macro2::TokenStream {
    let constraints = linearize(&f.constraints);

    let rent = constraints
        .iter()
        .any(|c| matches!(c, Constraint::RentExempt(ConstraintRentExempt::Enforce)))
        .then(|| quote! { let __anchor_rent = Rent::get()?; })
        .unwrap_or_else(|| quote! {});

    let checks: Vec<proc_macro2::TokenStream> = constraints
        .iter()
        .map(|c| generate_constraint(f, c))
        .collect();

    quote! {
        #rent
        #(#checks)*
    }
}

pub fn generate_composite(f: &CompositeField) -> proc_macro2::TokenStream {
    let checks: Vec<proc_macro2::TokenStream> = linearize(&f.constraints)
        .iter()
        .filter_map(|c| match c {
            Constraint::Raw(_) => Some(c),
            Constraint::Literal(_) => Some(c),
            _ => panic!("Invariant violation: composite constraints can only be raw or literals"),
        })
        .map(|c| generate_constraint_composite(f, c))
        .collect();
    quote! {
        #(#checks)*
    }
}

// Linearizes the constraint group so that constraints with dependencies
// run after those without.
pub fn linearize(c_group: &ConstraintGroup) -> Vec<Constraint> {
    let ConstraintGroup {
        init,
        zeroed,
        mutable,
        signer,
        has_one,
        literal,
        raw,
        owner,
        rent_exempt,
        seeds,
        executable,
        state,
        close,
        address,
        associated_token,
    } = c_group.clone();

    let mut constraints = Vec::new();

    if let Some(c) = zeroed {
        constraints.push(Constraint::Zeroed(c));
    }
    if let Some(c) = init {
        constraints.push(Constraint::Init(c));
    }
    if let Some(c) = seeds {
        constraints.push(Constraint::Seeds(c));
    }
    if let Some(c) = associated_token {
        constraints.push(Constraint::AssociatedToken(c));
    }
    if let Some(c) = mutable {
        constraints.push(Constraint::Mut(c));
    }
    if let Some(c) = signer {
        constraints.push(Constraint::Signer(c));
    }
    constraints.append(&mut has_one.into_iter().map(Constraint::HasOne).collect());
    constraints.append(&mut literal.into_iter().map(Constraint::Literal).collect());
    constraints.append(&mut raw.into_iter().map(Constraint::Raw).collect());
    if let Some(c) = owner {
        constraints.push(Constraint::Owner(c));
    }
    if let Some(c) = rent_exempt {
        constraints.push(Constraint::RentExempt(c));
    }
    if let Some(c) = executable {
        constraints.push(Constraint::Executable(c));
    }
    if let Some(c) = state {
        constraints.push(Constraint::State(c));
    }
    if let Some(c) = close {
        constraints.push(Constraint::Close(c));
    }
    if let Some(c) = address {
        constraints.push(Constraint::Address(c));
    }
    constraints
}

fn generate_constraint(f: &Field, c: &Constraint) -> proc_macro2::TokenStream {
    match c {
        Constraint::Init(c) => generate_constraint_init(f, c),
        Constraint::Zeroed(c) => generate_constraint_zeroed(f, c),
        Constraint::Mut(c) => generate_constraint_mut(f, c),
        Constraint::HasOne(c) => generate_constraint_has_one(f, c),
        Constraint::Signer(c) => generate_constraint_signer(f, c),
        Constraint::Literal(c) => generate_constraint_literal(&f.ident, c),
        Constraint::Raw(c) => generate_constraint_raw(&f.ident, c),
        Constraint::Owner(c) => generate_constraint_owner(f, c),
        Constraint::RentExempt(c) => generate_constraint_rent_exempt(f, c),
        Constraint::Seeds(c) => generate_constraint_seeds(f, c),
        Constraint::Executable(c) => generate_constraint_executable(f, c),
        Constraint::State(c) => generate_constraint_state(f, c),
        Constraint::Close(c) => generate_constraint_close(f, c),
        Constraint::Address(c) => generate_constraint_address(f, c),
        Constraint::AssociatedToken(c) => generate_constraint_associated_token(f, c),
    }
}

fn generate_constraint_composite(f: &CompositeField, c: &Constraint) -> proc_macro2::TokenStream {
    match c {
        Constraint::Raw(c) => generate_constraint_raw(&f.ident, c),
        Constraint::Literal(c) => generate_constraint_literal(&f.ident, c),
        _ => panic!("Invariant violation"),
    }
}

fn generate_constraint_address(f: &Field, c: &ConstraintAddress) -> proc_macro2::TokenStream {
    let field = &f.ident;
    let addr = &c.address;
    let error = generate_custom_error(field, &c.error, quote! { ConstraintAddress });
    quote! {
        if #field.key() != #addr {
            return #error;
        }
    }
}

pub fn generate_constraint_init(f: &Field, c: &ConstraintInitGroup) -> proc_macro2::TokenStream {
    generate_constraint_init_group(f, c)
}

pub fn generate_constraint_zeroed(f: &Field, _c: &ConstraintZeroed) -> proc_macro2::TokenStream {
    let field = &f.ident;
    let name_str = field.to_string();
    let ty_decl = f.ty_decl();
    let from_account_info = f.from_account_info_unchecked(None);
    quote! {
        let #field: #ty_decl = {
            let mut __data: &[u8] = &#field.try_borrow_data()?;
            let mut __disc_bytes = [0u8; 8];
            __disc_bytes.copy_from_slice(&__data[..8]);
            let __discriminator = u64::from_le_bytes(__disc_bytes);
            if __discriminator != 0 {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintZero, #name_str));
            }
            #from_account_info
        };
    }
}

pub fn generate_constraint_close(f: &Field, c: &ConstraintClose) -> proc_macro2::TokenStream {
    let field = &f.ident;
    let name_str = field.to_string();
    let target = &c.sol_dest;
    quote! {
        if #field.key() == #target.key() {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintClose, #name_str));
        }
    }
}

pub fn generate_constraint_mut(f: &Field, c: &ConstraintMut) -> proc_macro2::TokenStream {
    let ident = &f.ident;
    let error = generate_custom_error(ident, &c.error, quote! { ConstraintMut });
    quote! {
        if !#ident.to_account_info().is_writable {
            return #error;
        }
    }
}

pub fn generate_constraint_has_one(f: &Field, c: &ConstraintHasOne) -> proc_macro2::TokenStream {
    let target = c.join_target.clone();
    let ident = &f.ident;
    let field = match &f.ty {
        Ty::Loader(_) => quote! {#ident.load()?},
        Ty::AccountLoader(_) => quote! {#ident.load()?},
        _ => quote! {#ident},
    };
    let error = generate_custom_error(ident, &c.error, quote! { ConstraintHasOne });
    quote! {
        if #field.#target != #target.key() {
            return #error;
        }
    }
}

pub fn generate_constraint_signer(f: &Field, c: &ConstraintSigner) -> proc_macro2::TokenStream {
    let ident = &f.ident;
    let info = match f.ty {
        Ty::AccountInfo => quote! { #ident },
        Ty::ProgramAccount(_) => quote! { #ident.to_account_info() },
        Ty::Account(_) => quote! { #ident.to_account_info() },
        Ty::Loader(_) => quote! { #ident.to_account_info() },
        Ty::AccountLoader(_) => quote! { #ident.to_account_info() },
        Ty::CpiAccount(_) => quote! { #ident.to_account_info() },
        _ => panic!("Invalid syntax: signer cannot be specified."),
    };
    let error = generate_custom_error(ident, &c.error, quote! { ConstraintSigner });
    quote! {
        if !#info.is_signer {
            return #error;
        }
    }
}

pub fn generate_constraint_literal(
    ident: &Ident,
    c: &ConstraintLiteral,
) -> proc_macro2::TokenStream {
    let name_str = ident.to_string();
    let lit: proc_macro2::TokenStream = {
        let lit = &c.lit;
        let constraint = lit.value().replace('\"', "");
        let message = format!(
            "Deprecated. Should be used with constraint: #[account(constraint = {})]",
            constraint,
        );
        lit.span().warning(message).emit_as_item_tokens();
        constraint.parse().unwrap()
    };
    quote! {
        if !(#lit) {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::Deprecated, #name_str));
        }
    }
}

pub fn generate_constraint_raw(ident: &Ident, c: &ConstraintRaw) -> proc_macro2::TokenStream {
    let raw = &c.raw;
    let error = generate_custom_error(ident, &c.error, quote! { ConstraintRaw });
    quote! {
        if !(#raw) {
            return #error;
        }
    }
}

pub fn generate_constraint_owner(f: &Field, c: &ConstraintOwner) -> proc_macro2::TokenStream {
    let ident = &f.ident;
    let owner_address = &c.owner_address;
    let error = generate_custom_error(ident, &c.error, quote! { ConstraintOwner });
    quote! {
        if #ident.as_ref().owner != &#owner_address {
            return #error;
        }
    }
}

pub fn generate_constraint_rent_exempt(
    f: &Field,
    c: &ConstraintRentExempt,
) -> proc_macro2::TokenStream {
    let ident = &f.ident;
    let name_str = ident.to_string();
    let info = quote! {
        #ident.to_account_info()
    };
    match c {
        ConstraintRentExempt::Skip => quote! {},
        ConstraintRentExempt::Enforce => quote! {
            if !__anchor_rent.is_exempt(#info.lamports(), #info.try_data_len()?) {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintRentExempt, #name_str));
            }
        },
    }
}

fn generate_constraint_init_group(f: &Field, c: &ConstraintInitGroup) -> proc_macro2::TokenStream {
    let field = &f.ident;
    let name_str = f.ident.to_string();
    let ty_decl = f.ty_decl();
    let if_needed = if c.if_needed {
        quote! {true}
    } else {
        quote! {false}
    };
    let space = &c.space;

    // Payer for rent exemption.
    let payer = {
        let p = &c.payer;
        quote! {
            let payer = #p.to_account_info();
        }
    };

    // Convert from account info to account context wrapper type.
    let from_account_info = f.from_account_info_unchecked(Some(&c.kind));

    // PDA bump seeds.
    let (find_pda, seeds_with_bump) = match &c.seeds {
        None => (quote! {}, quote! {}),
        Some(c) => {
            let seeds = &mut c.seeds.clone();

            // If the seeds came with a trailing comma, we need to chop it off
            // before we interpolate them below.
            if let Some(pair) = seeds.pop() {
                seeds.push_value(pair.into_value());
            }

            let maybe_seeds_plus_comma = (!seeds.is_empty()).then(|| {
                quote! { #seeds, }
            });

            (
                quote! {
                    let (__pda_address, __bump) = Pubkey::find_program_address(
                        &[#maybe_seeds_plus_comma],
                        program_id,
                    );
                    __bumps.insert(#name_str.to_string(), __bump);
                },
                quote! {
                    &[
                        #maybe_seeds_plus_comma
                        &[__bump][..]
                    ][..]
                },
            )
        }
    };

    match &c.kind {
        InitKind::Token { owner, mint } => {
            let create_account = generate_create_account(
                field,
                quote! {anchor_spl::token::TokenAccount::LEN},
                quote! {&token_program.key()},
                seeds_with_bump,
            );
            quote! {
                // Define the bump and pda variable.
                #find_pda

                let #field: #ty_decl = {
                    if !#if_needed || #field.as_ref().owner == &anchor_lang::solana_program::system_program::ID {
                        // Define payer variable.
                        #payer

                        // Create the account with the system program.
                        #create_account

                        // Initialize the token account.
                        let cpi_program = token_program.to_account_info();
                        let accounts = anchor_spl::token::InitializeAccount {
                            account: #field.to_account_info(),
                            mint: #mint.to_account_info(),
                            authority: #owner.to_account_info(),
                            rent: rent.to_account_info(),
                        };
                        let cpi_ctx = anchor_lang::context::CpiContext::new(cpi_program, accounts);
                        anchor_spl::token::initialize_account(cpi_ctx)?;
                    }

                    let pa: #ty_decl = #from_account_info;
                    if #if_needed {
                        if pa.mint != #mint.key() {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintTokenMint, #name_str));
                        }
                        if pa.owner != #owner.key() {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintTokenOwner, #name_str));
                        }
                    }
                    pa
                };
            }
        }
        InitKind::AssociatedToken { owner, mint } => {
            quote! {
                // Define the bump and pda variable.
                #find_pda

                let #field: #ty_decl = {
                    if !#if_needed || #field.as_ref().owner == &anchor_lang::solana_program::system_program::ID {
                        #payer

                        let cpi_program = associated_token_program.to_account_info();
                        let cpi_accounts = anchor_spl::associated_token::Create {
                            payer: payer.to_account_info(),
                            associated_token: #field.to_account_info(),
                            authority: #owner.to_account_info(),
                            mint: #mint.to_account_info(),
                            system_program: system_program.to_account_info(),
                            token_program: token_program.to_account_info(),
                            rent: rent.to_account_info(),
                        };
                        let cpi_ctx = anchor_lang::context::CpiContext::new(cpi_program, cpi_accounts);
                        anchor_spl::associated_token::create(cpi_ctx)?;
                    }
                    let pa: #ty_decl = #from_account_info;
                    if #if_needed {
                        if pa.mint != #mint.key() {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintTokenMint, #name_str));
                        }
                        if pa.owner != #owner.key() {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintTokenOwner, #name_str));
                        }

                        if pa.key() != anchor_spl::associated_token::get_associated_token_address(&#owner.key(), &#mint.key()) {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::AccountNotAssociatedTokenAccount, #name_str));
                        }
                    }
                    pa
                };
            }
        }
        InitKind::Mint {
            owner,
            decimals,
            freeze_authority,
        } => {
            let create_account = generate_create_account(
                field,
                quote! {anchor_spl::token::Mint::LEN},
                quote! {&token_program.key()},
                seeds_with_bump,
            );
            let freeze_authority = match freeze_authority {
                Some(fa) => quote! { Option::<&anchor_lang::prelude::Pubkey>::Some(&#fa.key()) },
                None => quote! { Option::<&anchor_lang::prelude::Pubkey>::None },
            };
            quote! {
                // Define the bump and pda variable.
                #find_pda

                let #field: #ty_decl = {
                    if !#if_needed || #field.as_ref().owner == &anchor_lang::solana_program::system_program::ID {
                        // Define payer variable.
                        #payer

                        // Create the account with the system program.
                        #create_account

                        // Initialize the mint account.
                        let cpi_program = token_program.to_account_info();
                        let accounts = anchor_spl::token::InitializeMint {
                            mint: #field.to_account_info(),
                            rent: rent.to_account_info(),
                        };
                        let cpi_ctx = anchor_lang::context::CpiContext::new(cpi_program, accounts);
                        anchor_spl::token::initialize_mint(cpi_ctx, #decimals, &#owner.key(), #freeze_authority)?;
                    }
                    let pa: #ty_decl = #from_account_info;
                    if #if_needed {
                        if pa.mint_authority != anchor_lang::solana_program::program_option::COption::Some(#owner.key()) {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintMintMintAuthority, #name_str));
                        }
                        if pa.freeze_authority
                            .as_ref()
                            .map(|fa| #freeze_authority.as_ref().map(|expected_fa| fa != *expected_fa).unwrap_or(true))
                            .unwrap_or(#freeze_authority.is_some()) {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintMintFreezeAuthority, #name_str));
                        }
                        if pa.decimals != #decimals {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintMintDecimals, #name_str));
                        }
                    }
                    pa
                };
            }
        }
        InitKind::Program { owner } => {
            // Define the space variable.
            let space = match space {
                // If no explicit space param was given, serialize the type to bytes
                // and take the length (with +8 for the discriminator.)
                None => {
                    let account_ty = f.account_ty();
                    match matches!(f.ty, Ty::Loader(_) | Ty::AccountLoader(_)) {
                        false => {
                            quote! {
                                let space = 8 + #account_ty::default().try_to_vec().unwrap().len();
                            }
                        }
                        true => {
                            quote! {
                                let space = 8 + anchor_lang::__private::bytemuck::bytes_of(&#account_ty::default()).len();
                            }
                        }
                    }
                }
                // Explicit account size given. Use it.
                Some(s) => quote! {
                    let space = #s;
                },
            };

            // Define the owner of the account being created. If not specified,
            // default to the currently executing program.
            let owner = match owner {
                None => quote! {
                    program_id
                },
                Some(o) => quote! {
                    &#o
                },
            };

            // CPI to the system program to create the account.
            let create_account =
                generate_create_account(field, quote! {space}, owner.clone(), seeds_with_bump);

            // Put it all together.
            quote! {
                // Define the bump variable.
                #find_pda

                let #field = {
                    let actual_field = #field.to_account_info();
                    let actual_owner = actual_field.owner;

                    // Define the account space variable.
                    #space

                    // Create the account. Always do this in the event
                    // if needed is not specified or the system program is the owner.
                    if !#if_needed || actual_owner == &anchor_lang::solana_program::system_program::ID {
                        // Define the payer variable.
                        #payer

                        // CPI to the system program to create.
                        #create_account
                    }

                    // Convert from account info to account context wrapper type.
                    let pa: #ty_decl = #from_account_info;

                    // Assert the account was created correctly.
                    if #if_needed {
                        if space != actual_field.data_len() {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSpace, #name_str));
                        }

                        if actual_owner != #owner {
                            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintOwner, #name_str));
                        }

                        {
                            let required_lamports = __anchor_rent.minimum_balance(space);
                            if pa.to_account_info().lamports() < required_lamports {
                                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintRentExempt, #name_str));
                            }
                        }
                    }

                    // Done.
                    pa
                };
            }
        }
    }
}

fn generate_constraint_seeds(f: &Field, c: &ConstraintSeedsGroup) -> proc_macro2::TokenStream {
    let name = &f.ident;
    let name_str = name.to_string();

    let s = &mut c.seeds.clone();

    let deriving_program_id = c
        .program_seed
        .clone()
        // If they specified a seeds::program to use when deriving the PDA, use it.
        .map(|program_id| quote! { #program_id })
        // Otherwise fall back to the current program's program_id.
        .unwrap_or(quote! { program_id });

    // If the seeds came with a trailing comma, we need to chop it off
    // before we interpolate them below.
    if let Some(pair) = s.pop() {
        s.push_value(pair.into_value());
    }

    // If the bump is provided with init *and target*, then force it to be the
    // canonical bump.
    //
    // Note that for `#[account(init, seeds)]`, find_program_address has already
    // been run in the init constraint.
    if c.is_init && c.bump.is_some() {
        let b = c.bump.as_ref().unwrap();
        quote! {
            if #name.key() != __pda_address {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSeeds, #name_str));
            }
            if __bump != #b {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSeeds, #name_str));
            }
        }
    }
    // Init seeds but no bump. We already used the canonical to create bump so
    // just check the address.
    //
    // Note that for `#[account(init, seeds)]`, find_program_address has already
    // been run in the init constraint.
    else if c.is_init {
        quote! {
            if #name.key() != __pda_address {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSeeds, #name_str));
            }
        }
    }
    // No init. So we just check the address.
    else {
        let maybe_seeds_plus_comma = (!s.is_empty()).then(|| {
            quote! { #s, }
        });

        let define_pda = match c.bump.as_ref() {
            // Bump target not given. Find it.
            None => quote! {
                let (__pda_address, __bump) = Pubkey::find_program_address(
                    &[#maybe_seeds_plus_comma],
                    &#deriving_program_id,
                );
                __bumps.insert(#name_str.to_string(), __bump);
            },
            // Bump target given. Use it.
            Some(b) => quote! {
                let __pda_address = Pubkey::create_program_address(
                    &[#maybe_seeds_plus_comma &[#b][..]],
                    &#deriving_program_id,
                ).map_err(|_| anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSeeds, #name_str))?;
            },
        };
        quote! {
            // Define the PDA.
            #define_pda

            // Check it.
            if #name.key() != __pda_address {
                return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintSeeds, #name_str));
            }
        }
    }
}

fn generate_constraint_associated_token(
    f: &Field,
    c: &ConstraintAssociatedToken,
) -> proc_macro2::TokenStream {
    let name = &f.ident;
    let name_str = name.to_string();
    let wallet_address = &c.wallet;
    let spl_token_mint_address = &c.mint;
    quote! {
        if #name.owner != #wallet_address.key() {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintTokenOwner, #name_str));
        }
        let __associated_token_address = anchor_spl::associated_token::get_associated_token_address(&#wallet_address.key(), &#spl_token_mint_address.key());
        if #name.key() != __associated_token_address {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintAssociated, #name_str));
        }
    }
}

// Generated code to create an account with with system program with the
// given `space` amount of data, owned by `owner`.
//
// `seeds_with_nonce` should be given for creating PDAs. Otherwise it's an
// empty stream.
pub fn generate_create_account(
    field: &Ident,
    space: proc_macro2::TokenStream,
    owner: proc_macro2::TokenStream,
    seeds_with_nonce: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        // If the account being initialized already has lamports, then
        // return them all back to the payer so that the account has
        // zero lamports when the system program's create instruction
        // is eventually called.
        let __current_lamports = #field.lamports();
        if __current_lamports == 0 {
            // Create the token account with right amount of lamports and space, and the correct owner.
            let lamports = __anchor_rent.minimum_balance(#space);
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::create_account(
                    &payer.key(),
                    &#field.key(),
                    lamports,
                    #space as u64,
                    #owner,
                ),
                &[
                    payer.to_account_info(),
                    #field.to_account_info(),
                    system_program.to_account_info(),
                ],
                &[#seeds_with_nonce],
            )?;
        } else {
            // Fund the account for rent exemption.
            let required_lamports = __anchor_rent
                .minimum_balance(#space)
                .max(1)
                .saturating_sub(__current_lamports);
            if required_lamports > 0 {
                anchor_lang::solana_program::program::invoke(
                    &anchor_lang::solana_program::system_instruction::transfer(
                        &payer.key(),
                        &#field.key(),
                        required_lamports,
                    ),
                    &[
                        payer.to_account_info(),
                        #field.to_account_info(),
                        system_program.to_account_info(),
                    ],
                )?;
            }
            // Allocate space.
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::allocate(
                    &#field.key(),
                    #space as u64,
                ),
                &[
                    #field.to_account_info(),
                    system_program.to_account_info(),
                ],
                &[#seeds_with_nonce],
            )?;
            // Assign to the spl token program.
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::assign(
                    &#field.key(),
                    #owner,
                ),
                &[
                    #field.to_account_info(),
                    system_program.to_account_info(),
                ],
                &[#seeds_with_nonce],
            )?;
        }
    }
}

pub fn generate_constraint_executable(
    f: &Field,
    _c: &ConstraintExecutable,
) -> proc_macro2::TokenStream {
    let name = &f.ident;
    let name_str = name.to_string();
    quote! {
        if !#name.to_account_info().executable {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintExecutable, #name_str));
        }
    }
}

pub fn generate_constraint_state(f: &Field, c: &ConstraintState) -> proc_macro2::TokenStream {
    let program_target = c.program_target.clone();
    let ident = &f.ident;
    let name_str = ident.to_string();
    let account_ty = match &f.ty {
        Ty::CpiState(ty) => &ty.account_type_path,
        _ => panic!("Invalid state constraint"),
    };
    quote! {
        // Checks the given state account is the canonical state account for
        // the target program.
        if #ident.key() != anchor_lang::accounts::cpi_state::CpiState::<#account_ty>::address(&#program_target.key()) {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintState, #name_str));
        }
        if #ident.as_ref().owner != &#program_target.key() {
            return Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::ConstraintState, #name_str));
        }
    }
}

fn generate_custom_error(
    account_name: &Ident,
    custom_error: &Option<Expr>,
    error: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let account_name = account_name.to_string();
    match custom_error {
        Some(error) => {
            quote! { Err(anchor_lang::anchor_attribute_error::error_with_account_name!(#error, #account_name)) }
        }
        None => {
            quote! { Err(anchor_lang::anchor_attribute_error::error_with_account_name!(anchor_lang::error::ErrorCode::#error, #account_name)) }
        }
    }
}
